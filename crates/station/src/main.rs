//! Station — the RetroOS compositor.
//!
//!   station [--headless WxH] [--personality text|desktop] [--exec CMD] [--spawn CMD]…
//!           [--test-ipc] [--screenshot-after MS PATH]
//!
//! `--exec` starts the primary client (normally retro-console); when it exits
//! for good, Station exits with its status. On real hardware Station runs
//! inside a logind session (libseat) and drives the display directly.

mod broker;
mod chrome;
mod desktop;
mod handlers;
mod headless;
mod inject;
mod input;
mod layers;
mod render;
mod state;
mod tasklist;
mod tasks;
mod udev;

use render::Scene;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::calloop::EventLoop;
use smithay::reexports::wayland_server::Display;
use state::{Personality, Station};
use std::path::Path;
use std::time::Duration;

pub enum Backend {
    None,
    Headless(Box<headless::HeadlessData>),
    Udev(Box<udev::UdevData>),
}

/// Frame callbacks + housekeeping after a render.
pub fn send_frames(st: &mut Station) {
    if let Some(output) = st.output.clone() {
        let now = st.start.elapsed();
        // Minimised windows draw nothing until they come back.
        let hidden: Vec<smithay::desktop::Window> = st
            .tasks
            .iter()
            .filter(|t| st.personality == Personality::Desktop && t.place.minimized)
            .map(|t| t.window.clone())
            .collect();
        for w in st.space.elements() {
            if hidden.contains(w) {
                continue;
            }
            w.send_frame(&output, now, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        st.layer_frames(&output, now);
        smithay::desktop::layer_map_for_output(&output).cleanup();
    }
    st.space.refresh();
    st.popups.cleanup();
    let _ = st.dh.flush_clients();
}

pub fn change_vt(st: &mut Station, vt: i32) {
    udev::change_vt(st, vt);
}

pub fn screenshot(st: &mut Station, path: &Path) -> Result<(), String> {
    let output = st.output.clone().ok_or("no output")?;
    st.prepare_frame();
    let canvas = match &mut st.backend {
        Backend::Headless(_) => headless::screenshot(st),
        Backend::Udev(ud) => {
            let scene = Scene {
                space: &st.space,
                tasks: &st.tasks,
                cursor_status: &st.cursor_status,
                pointer: st.pointer_location,
                scale: st.scale,
                art_scale: st.art_scale,
                show_pointer: false,
                overlay: st.task_list.as_ref().and_then(|t| t.buffer()),
                personality: st.personality,
                desktop: &st.desktop,
            };
            match ud.gpu.as_mut().ok_or("no renderer")? {
                udev::Gpu::Pixman { renderer, .. } => {
                    render::offscreen(renderer, &output, &scene, &mut ud.cursor)?
                }
                udev::Gpu::Gles { renderer, .. } => {
                    render::offscreen_gles(renderer, &output, &scene, &mut ud.cursor)?
                }
            }
        }
        Backend::None => return Err("no backend".into()),
    };
    canvas.save_png(path).map_err(|e| e.to_string())
}

fn split_cmd(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("station=info,smithay=warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    let mut headless: Option<(i32, i32)> = None;
    let mut exec: Option<String> = None;
    let mut spawns: Vec<String> = Vec::new();
    let mut test_ipc = false;
    let mut personality = Personality::Text;
    let mut shot: Option<(u64, String)> = None;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let next = |i: usize| args.get(i + 1).cloned().unwrap_or_default();
        match args[i].as_str() {
            "--headless" => {
                let v = next(i);
                let (w, h) = v.split_once('x').unwrap_or(("1920", "1080"));
                headless = Some((w.parse().unwrap_or(1920), h.parse().unwrap_or(1080)));
                i += 1;
            }
            "--exec" => {
                exec = Some(next(i));
                i += 1;
            }
            "--spawn" => {
                spawns.push(next(i));
                i += 1;
            }
            "--personality" => {
                personality = if next(i) == "desktop" {
                    Personality::Desktop
                } else {
                    Personality::Text
                };
                i += 1;
            }
            "--test-ipc" => test_ipc = true,
            "--screenshot-after" => {
                shot = Some((
                    next(i).parse().unwrap_or(2000),
                    args.get(i + 2).cloned().unwrap_or_default(),
                ));
                i += 2;
            }
            "--version" => {
                println!("station {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("station: unknown argument {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let mut event_loop: EventLoop<'static, Station> = EventLoop::try_new().expect("event loop");
    let display: Display<Station> = Display::new().expect("wayland display");
    let mut st = Station::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        personality,
        Backend::None,
    );
    st.broker.test_ipc = test_ipc;

    match headless {
        Some((w, h)) => headless::init(&mut st, w, h),
        None => {
            if let Err(e) = udev::init(&mut st) {
                tracing::error!("cannot start on this display: {e}");
                std::process::exit(1);
            }
        }
    }
    tracing::info!(socket = ?st.socket_name, broker = %st.broker.path().display(), "Station ready");
    eprintln!("RETRO-STATION: ready {}", st.socket_name.to_string_lossy());

    if let Some(cmd) = exec {
        match st.spawn(&split_cmd(&cmd), None, &[], None) {
            Ok(pid) => st.primary_pid = Some(pid),
            Err(e) => {
                tracing::error!("cannot start {cmd}: {e}");
                std::process::exit(1);
            }
        }
    }
    st.ensure_bar();
    for s in spawns {
        if let Err(e) = st.spawn(&split_cmd(&s), None, &[], None) {
            tracing::warn!("cannot start {s}: {e}");
        }
    }

    let h = event_loop.handle();
    h.insert_source(
        Timer::from_duration(Duration::from_millis(250)),
        |_, _, st| {
            st.reap();
            TimeoutAction::ToDuration(Duration::from_millis(250))
        },
    )
    .expect("reaper");
    if let Some((ms, path)) = shot {
        h.insert_source(
            Timer::from_duration(Duration::from_millis(ms)),
            move |_, _, st| {
                match screenshot(st, Path::new(&path)) {
                    Ok(()) => tracing::info!("screenshot saved to {path}"),
                    Err(e) => tracing::error!("screenshot: {e}"),
                }
                st.loop_signal.stop();
                TimeoutAction::Drop
            },
        )
        .expect("screenshot timer");
    }

    event_loop
        .run(None, &mut st, |st| {
            let _ = st.dh.flush_clients();
        })
        .expect("event loop");
    // Clean up clients still running.
    for (c, _) in st.children.iter_mut() {
        let _ = c.kill();
    }
    std::process::exit(st.exit_code);
}
