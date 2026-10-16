//! retro-post — "Phosphor BIOS", the first thing RetroOS draws.
//!
//! Runs early in boot as a short-lived system service: lights the display,
//! plays the startup chime, shows a BIOS-style POST screen made of real
//! hardware data, and offers the F8 Startup Menu and DEL Setup. It never
//! delays boot by more than about a second and never fails boot: without a
//! display it simply exits.
//!
//!   retro-post                     normal boot
//!   retro-post --png OUT [SCREEN]  render a screen with fixed data (post|menu|setup)

mod config;
mod probe;

use config::Config;
use probe::SysInfo;
use retro_gfx::font::phosphor_mono;
use retro_gfx::grid::{fit_scale, Grid};
use retro_gfx::palette::{self, Scheme};
use retro_gfx::Canvas;
use retro_kms::{Display, Key, Keyboard};
use std::path::Path;
use std::time::{Duration, Instant};

const COLS: u32 = 80;
const ROWS: u32 = 25;

// Phosphor16 indices used on these screens.
const BLACK: u8 = 0;
const COBALT: u8 = 1;
const TEAL: u8 = 3;
const SILVER: u8 = 7;
const SLATE: u8 = 8;
const LIME: u8 = 10;
const LEMON: u8 = 14;
const WHITE: u8 = 15;

/// The BIOS look: Silver on Black.
fn bios_scheme() -> Scheme {
    Scheme {
        name: "BIOS",
        palette: palette::PHOSPHOR16,
        fg: SILVER,
        bg: BLACK,
        border: BLACK,
        hot: LEMON,
        sel: COBALT,
    }
}

/// What the POST screen shows at a given moment of the animation.
struct PostState<'a> {
    info: &'a SysInfo,
    mem_shown: u64,
    mem_done: bool,
    lines_shown: usize,
}

fn device_lines(info: &SysInfo) -> Vec<(String, String)> {
    let mut v = vec![("Video".to_string(), info.video.clone())];
    if !info.storage.is_empty() {
        v.push(("Storage".into(), info.storage.join(", ")));
    }
    v.push(("Input".into(), info.input.clone()));
    if !info.network.is_empty() {
        v.push(("Network".into(), info.network.join(", ")));
    }
    v
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut o = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            o.push(',');
        }
        o.push(c);
    }
    o
}

fn post_grid(st: &PostState) -> Grid {
    let mut g = Grid::new(COLS, ROWS, &bios_scheme());
    g.put(1, 1, "Phosphor BIOS v1.0", WHITE, BLACK);
    g.text(1, 2, "Copyright (C) 1987-1993 Phosphor Computer Company");
    g.put(1, 4, "RetroOS 1.0", LEMON, BLACK);
    g.text(1, 6, &st.info.model);
    g.text(1, 7, &st.info.cpu);
    let mem = format!("Memory Test :  {:>12}K", thousands(st.mem_shown));
    let x = g.text(1, 9, &mem);
    if st.mem_done {
        g.put(x + 1, 9, "OK", LIME, BLACK);
    }
    for (i, (k, v)) in device_lines(st.info)
        .iter()
        .enumerate()
        .take(st.lines_shown)
    {
        g.text(1, 11 + i as u32, &format!("{k:<12}:  {v}"));
    }
    g.put(1, ROWS - 2, "Press ", SILVER, BLACK);
    g.put(7, ROWS - 2, "F8", WHITE, BLACK);
    g.put(9, ROWS - 2, " for Startup Menu, ", SILVER, BLACK);
    g.put(28, ROWS - 2, "DEL", WHITE, BLACK);
    g.put(31, ROWS - 2, " for Setup", SILVER, BLACK);
    g
}

/// The Phosphor logo: a glowing phosphor dot (concentric diamonds), top right.
fn draw_logo(c: &mut Canvas, g: &Grid, scale: u32) {
    let font = phosphor_mono();
    let (ox, oy, cw, ch) = g.geometry(font, c, scale);
    let size = 12i32; // diamond radius in logo pixels
    let px = scale as i32 * 2;
    let cx = ox + (COLS as i32 - 9) * cw as i32;
    let cy = oy + 2 * ch as i32;
    let mut s = c.surface();
    let rings = [
        (size, palette::COBALT),
        (size - 3, palette::SKY),
        (size - 6, palette::AQUA),
        (size - 9, palette::WHITE),
    ];
    for (r, col) in rings {
        for dy in -r..=r {
            let w = r - dy.abs();
            s.fill_rect(
                cx - w * px,
                cy + dy * px,
                ((2 * w + 1) * px) as u32,
                px as u32,
                col,
            );
        }
    }
    let label = "PHOSPHOR";
    let lx = cx - (label.len() as i32 * cw as i32) / 2 + px / 2;
    s.text(
        font,
        lx,
        cy + (size + 2) * px,
        scale,
        palette::LEMON,
        None,
        label,
    );
}

fn render_post(st: &PostState, c: &mut Canvas, scale: u32) {
    let g = post_grid(st);
    g.render(phosphor_mono(), c, scale);
    draw_logo(c, &g, scale);
}

// ---------------------------------------------------------------- menu

const MENU: [&str; 6] = [
    "RetroDOS prompt",
    "RetroOS Desktop",
    "Safe mode (POSIX shell)",
    "Safe mode with networking",
    "Restart",
    "Shut down",
];

fn menu_grid(sel: usize, remaining: Option<u32>) -> Grid {
    let mut g = Grid::new(COLS, ROWS, &bios_scheme());
    g.put(2, 2, "RetroOS Startup Menu", WHITE, BLACK);
    g.text(2, 3, "════════════════════");
    for (i, item) in MENU.iter().enumerate() {
        let y = 6 + i as u32;
        let line = format!("  {}. {:<32}", i + 1, item);
        if i == sel {
            g.put(4, y, &line, WHITE, COBALT);
        } else {
            g.put(4, y, &line, SILVER, BLACK);
        }
    }
    let x = g.text(2, 14, "Enter a choice: ");
    g.put(x, 14, &(sel + 1).to_string(), WHITE, BLACK);
    if let Some(t) = remaining {
        g.text(50, 14, &format!("Time remaining: {t:>3}"));
    }
    g.put(
        2,
        ROWS - 2,
        "↑↓ select   Enter start   F8 cancel",
        SLATE,
        BLACK,
    );
    g
}

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Text,
    Desktop,
    Safe,
    SafeNet,
    Restart,
    Shutdown,
}

fn run_menu(
    d: &mut Display,
    kb: &mut Keyboard,
    c: &mut Canvas,
    scale: u32,
    cfg: &Config,
) -> Option<Choice> {
    let mut sel = if cfg.mode == "desktop" { 1 } else { 0 };
    let deadline = Instant::now() + Duration::from_secs(cfg.menu_timeout as u64);
    let mut counting = true;
    loop {
        let remaining = counting
            .then(|| deadline.saturating_duration_since(Instant::now()).as_secs() as u32 + 1);
        menu_grid(sel, remaining).render(phosphor_mono(), c, scale);
        let _ = d.present(c);
        if counting && Instant::now() >= deadline {
            break;
        }
        let Some(k) = kb.next(Duration::from_millis(250)) else {
            continue;
        };
        counting = false; // any key stops the countdown, like MS-DOS 6
        match k {
            Key::Up => sel = (sel + MENU.len() - 1) % MENU.len(),
            Key::Down => sel = (sel + 1) % MENU.len(),
            Key::Digit(n) if (1..=MENU.len() as u8).contains(&n) => sel = n as usize - 1,
            Key::Enter => break,
            Key::F(8) | Key::Esc => return None,
            _ => {}
        }
    }
    Some(match sel {
        0 => Choice::Text,
        1 => Choice::Desktop,
        2 => Choice::Safe,
        3 => Choice::SafeNet,
        4 => Choice::Restart,
        _ => Choice::Shutdown,
    })
}

// ---------------------------------------------------------------- setup

fn setup_grid(cfg: &Config, sel: usize, saved: bool) -> Grid {
    let mut g = Grid::new(COLS, ROWS, &Scheme::CLASSIC);
    g.fill(0, 0, COLS, 1, ' ', BLACK, TEAL);
    g.put(2, 0, "Phosphor BIOS Setup — Startup", BLACK, TEAL);
    g.dialog(10, 4, 60, 13, "Startup Options", SILVER, COBALT, true);
    let on = |b: bool| if b { "Yes" } else { "No" };
    let rows = [
        (
            "Start in",
            if cfg.mode == "desktop" {
                "RetroOS Desktop"
            } else {
                "RetroDOS prompt"
            }
            .to_string(),
        ),
        ("Fast POST", on(cfg.fast_post).to_string()),
        ("Startup chime", on(cfg.chime).to_string()),
        (
            "Startup Menu timeout",
            format!("{} seconds", cfg.menu_timeout),
        ),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        let y = 6 + i as u32 * 2;
        g.put(13, y, &format!("{k:<24}"), WHITE, COBALT);
        let val = format!("[{v:^17}]");
        if i == sel {
            g.put(40, y, &val, BLACK, TEAL);
        } else {
            g.put(40, y, &val, LEMON, COBALT);
        }
    }
    let msg = if saved { "Settings saved." } else { "" };
    g.put(13, 15, msg, LIME, COBALT);
    g.fill(0, ROWS - 1, COLS, 1, ' ', BLACK, TEAL);
    g.put(
        1,
        ROWS - 1,
        "↑↓ Select  Space Change  F10 Save and continue  Esc Continue without saving",
        BLACK,
        TEAL,
    );
    g
}

fn run_setup(d: &mut Display, kb: &mut Keyboard, c: &mut Canvas, scale: u32, cfg: &mut Config) {
    let mut sel = 0;
    let mut work = cfg.clone();
    loop {
        setup_grid(&work, sel, false).render(phosphor_mono(), c, scale);
        let _ = d.present(c);
        let Some(k) = kb.next(Duration::from_secs(60)) else {
            continue;
        };
        match k {
            Key::Up => sel = (sel + 3) % 4,
            Key::Down => sel = (sel + 1) % 4,
            Key::Space | Key::Enter => match sel {
                0 => {
                    work.mode = if work.mode == "desktop" {
                        "text".into()
                    } else {
                        "desktop".into()
                    }
                }
                1 => work.fast_post = !work.fast_post,
                2 => work.chime = !work.chime,
                _ => {
                    let steps = [5, 10, 30, 60];
                    let i = steps
                        .iter()
                        .position(|s| *s == work.menu_timeout)
                        .map(|i| (i + 1) % steps.len())
                        .unwrap_or(2);
                    work.menu_timeout = steps[i];
                }
            },
            Key::F(10) => {
                if work.save(Path::new(config::PATH)).is_ok() {
                    *cfg = work;
                    setup_grid(cfg, sel, true).render(phosphor_mono(), c, scale);
                    let _ = d.present(c);
                    std::thread::sleep(Duration::from_millis(600));
                }
                return;
            }
            Key::Esc => return,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- boot

fn uptime_ms() -> u64 {
    std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|v| (v * 1000.0) as u64)
        .unwrap_or(0)
}

/// Log a marker to the kernel log (seen on the serial console by the test harness).
fn marker(msg: &str) {
    let line = format!("<6>{msg}\n");
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
    eprintln!("{msg}");
}

fn play_chime() {
    for p in ["/usr/share/retroos/sounds/startup.wav"] {
        if Path::new(p).exists() {
            let _ = std::process::Command::new("aplay")
                .args(["-q", p])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            return;
        }
    }
}

fn apply(choice: Choice) {
    let write = |s: &str| {
        let _ = std::fs::create_dir_all("/run/retroos");
        let _ = std::fs::write(config::CHOICE, s);
    };
    let systemctl = |args: &[&str]| {
        let _ = std::process::Command::new("systemctl").args(args).status();
    };
    match choice {
        Choice::Text => write("text\n"),
        Choice::Desktop => write("desktop\n"),
        Choice::Safe => {
            write("safe\n");
            systemctl(&["--no-block", "isolate", "retro-safe.target"]);
        }
        Choice::SafeNet => {
            write("safe-net\n");
            systemctl(&["--no-block", "isolate", "retro-safe-net.target"]);
        }
        Choice::Restart => systemctl(&["--no-block", "reboot"]),
        Choice::Shutdown => systemctl(&["--no-block", "poweroff"]),
    }
}

fn wait_for_display() -> Option<Display> {
    // The DRM device may still be probing this early in boot.
    let start = Instant::now();
    loop {
        match Display::open() {
            Ok(d) => return Some(d),
            Err(e) if start.elapsed() > Duration::from_secs(3) => {
                eprintln!("retro-post: no display ({e}); continuing boot");
                return None;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn boot() {
    let t0 = Instant::now();
    let mut cfg = Config::load(Path::new(config::PATH));
    if cfg.chime {
        play_chime();
    }
    let Some(mut d) = wait_for_display() else {
        marker(&format!("RETRO-POST: done t={} display=none", uptime_ms()));
        return;
    };
    let mut kb = Keyboard::open();
    let (w, h) = (d.width, d.height);
    let scale = fit_scale(w, h, 8, 16, COLS, ROWS);
    let mut c = Canvas::new(w, h, 0);
    let info = probe::probe(&format!("{} {w}x{h}", d.connector_name));
    marker(&format!(
        "RETRO-POST: start t={} {}x{} scale={scale}",
        uptime_ms(),
        w,
        h
    ));

    let (count_ms, line_ms, hold_ms) = if cfg.fast_post {
        (450u64, 45u64, 350u64)
    } else {
        (1600, 180, 1200)
    };
    let lines = device_lines(&info).len();
    let total = count_ms + line_ms * lines as u64 + hold_ms;
    let mut wanted: Option<Key> = None;
    loop {
        let el = t0.elapsed().as_millis() as u64;
        let frac = (el as f64 / count_ms as f64).min(1.0);
        // Count in 1 MB-ish steps like a real memory test.
        let mem_shown = ((info.mem_kb as f64 * frac) as u64 / 1024) * 1024;
        let st = PostState {
            info: &info,
            mem_shown: if frac >= 1.0 { info.mem_kb } else { mem_shown },
            mem_done: frac >= 1.0,
            lines_shown: if el < count_ms {
                0
            } else {
                (((el - count_ms) / line_ms) as usize + 1).min(lines)
            },
        };
        render_post(&st, &mut c, scale);
        let _ = d.present(&c);
        if el >= total || wanted.is_some() {
            break;
        }
        if kb.count() == 0 && el % 500 < 40 {
            kb.rescan();
        }
        match kb.next(Duration::from_millis(33)) {
            Some(k @ (Key::F(8) | Key::Delete)) => wanted = Some(k),
            Some(Key::Esc) if el < count_ms => {} // can't skip the memory test, but it's quick
            _ => {}
        }
    }
    match wanted {
        Some(Key::F(8)) => {
            if let Some(choice) = run_menu(&mut d, &mut kb, &mut c, scale, &cfg) {
                apply(choice);
            }
        }
        Some(Key::Delete) => run_setup(&mut d, &mut kb, &mut c, scale, &mut cfg),
        _ => {}
    }
    // Leave the screen black for the session's compositor.
    c.surface().clear(0);
    let _ = d.present(&c);
    marker(&format!("RETRO-POST: done t={}", uptime_ms()));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--png") {
        let out = args.get(2).cloned().unwrap_or_else(|| "post.png".into());
        let screen = args.get(3).map(String::as_str).unwrap_or("post");
        let (w, h) = (1920, 1080);
        let scale = fit_scale(w, h, 8, 16, COLS, ROWS);
        let mut c = Canvas::new(w, h, 0);
        let info = probe::fake();
        match screen {
            "menu" => menu_grid(0, Some(30)).render(phosphor_mono(), &mut c, scale),
            "setup" => {
                setup_grid(&Config::default(), 0, false).render(phosphor_mono(), &mut c, scale)
            }
            _ => render_post(
                &PostState {
                    info: &info,
                    mem_shown: info.mem_kb,
                    mem_done: true,
                    lines_shown: 9,
                },
                &mut c,
                scale,
            ),
        }
        if let Err(e) = c.save_png(&out) {
            eprintln!("retro-post: {e}");
            std::process::exit(1);
        }
        return;
    }
    boot();
}
