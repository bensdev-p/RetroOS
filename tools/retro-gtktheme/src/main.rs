//! retro-gtktheme THEMES_DIR: writes the Phosphor GTK themes, one per
//! RetroOS colour scheme (D-056): THEMES_DIR/Phosphor-<scheme>/ with
//! index.theme, gtk-3.0/gtk.css (+ assets/*.png) and gtk-4.0/gtk.css, plus
//! THEMES_DIR/Phosphor (Classic). The colours come from retro_gfx::Theme,
//! so programs from elsewhere (Firefox, GTK and, through GTK, Chromium)
//! wear the same faces, bevels and selection as RetroOS's own windows.
//! Station names the theme of the scheme in use (GTK_THEME) for every
//! program it starts. Installed as /usr/share/themes.

use retro_gfx::{Rgb, Scheme, Theme};
use std::path::{Path, PathBuf};

fn hex(c: Rgb) -> String {
    format!("#{:06x}", c & 0xFF_FFFF)
}

/// The colours a theme's CSS uses, by role.
struct Colors {
    face: String,
    text: String,
    disabled: String,
    line: String,
    light: String,
    dark: String,
    well: String,
    active: String,
    active_text: String,
    inactive: String,
    inactive_text: String,
    sel: String,
    sel_text: String,
}

impl Colors {
    fn of(t: &Theme) -> Colors {
        Colors {
            face: hex(t.face),
            text: hex(t.text),
            disabled: hex(t.disabled),
            line: hex(t.line),
            light: hex(t.light),
            dark: hex(t.dark),
            well: hex(t.well),
            active: hex(t.active),
            active_text: hex(t.active_text),
            inactive: hex(t.inactive),
            inactive_text: hex(t.inactive_text),
            sel: hex(t.sel),
            sel_text: hex(t.sel_text),
        }
    }
}

/// The named colours GTK programs and Firefox read (@theme_bg_color …).
fn named_colors(c: &Colors) -> String {
    let pairs = [
        ("theme_bg_color", &c.face),
        ("theme_fg_color", &c.text),
        ("theme_base_color", &c.well),
        ("theme_text_color", &c.text),
        ("theme_selected_bg_color", &c.sel),
        ("theme_selected_fg_color", &c.sel_text),
        ("theme_unfocused_bg_color", &c.face),
        ("theme_unfocused_fg_color", &c.text),
        ("theme_unfocused_base_color", &c.well),
        ("theme_unfocused_text_color", &c.text),
        ("theme_unfocused_selected_bg_color", &c.sel),
        ("theme_unfocused_selected_fg_color", &c.sel_text),
        ("insensitive_bg_color", &c.face),
        ("insensitive_fg_color", &c.disabled),
        ("insensitive_base_color", &c.face),
        ("borders", &c.line),
        ("unfocused_borders", &c.line),
        ("wm_title", &c.active_text),
        ("wm_unfocused_title", &c.inactive_text),
        ("wm_bg", &c.active),
        ("wm_unfocused_bg", &c.inactive),
        ("accent_color", &c.sel),
        ("accent_bg_color", &c.sel),
        ("accent_fg_color", &c.sel_text),
        ("window_bg_color", &c.face),
        ("window_fg_color", &c.text),
        ("view_bg_color", &c.well),
        ("view_fg_color", &c.text),
        ("headerbar_bg_color", &c.active),
        ("headerbar_fg_color", &c.active_text),
        ("popover_bg_color", &c.face),
        ("popover_fg_color", &c.text),
        ("card_bg_color", &c.well),
        ("card_fg_color", &c.text),
        ("dialog_bg_color", &c.face),
        ("dialog_fg_color", &c.text),
    ];
    let mut s: String = pairs
        .iter()
        .map(|(n, v)| format!("@define-color {n} {v};\n"))
        .collect();
    s.push_str("@define-color warning_color #a05800;\n@define-color error_color #a80000;\n@define-color success_color #007800;\n");
    s
}

/// Bevels as border colours: raised (light top-left) or sunken.
fn raised(c: &Colors) -> String {
    format!("{} {} {} {}", c.light, c.dark, c.dark, c.light)
}

fn sunken(c: &Colors) -> String {
    format!("{} {} {} {}", c.dark, c.light, c.light, c.dark)
}

/// The rules both GTK versions understand.
fn common_css(c: &Colors) -> String {
    let (face, text, dis, line, well) = (&c.face, &c.text, &c.disabled, &c.line, &c.well);
    let (sel, sel_text) = (&c.sel, &c.sel_text);
    let (up, down) = (raised(c), sunken(c));
    format!(
        r#"
* {{
  border-radius: 0;
  box-shadow: none;
  text-shadow: none;
  -gtk-icon-shadow: none;
  transition: none;
  background-image: none;
}}

window, .background, dialog, messagedialog, popover, popover > contents,
.titlebar:not(headerbar) {{
  background-color: {face};
  color: {text};
}}

/* Lantern, RetroOS's system font, at its own size (13 px, 26 px at 2x):
   every pixel of it lands on a screen pixel (D-056). */
window, menu, .menu, popover, tooltip, .tooltip {{
  font-family: "Lantern", sans-serif;
  font-size: 13px;
}}

label:disabled, *:disabled {{ color: {dis}; }}

/* Buttons: raised, a black outline, pressed in when held (STYLE.md). */
button, button.text-button, button.image-button, combobox button,
spinbutton button, notebook tab, treeview header button {{
  background-color: {face};
  color: {text};
  border: 2px solid;
  border-color: {up};
  padding: 2px 8px;
  min-height: 18px;
  min-width: 16px;
}}
button {{ box-shadow: 0 0 0 1px {line}; margin: 1px; }}
button:hover {{ background-color: {face}; }}
button:active, button:checked, button.toggle:checked {{
  border-color: {down};
  padding: 3px 7px 1px 9px;
}}
button:disabled {{ color: {dis}; }}
button.flat, button.image-button.flat, toolbar button, .toolbar button {{
  border-color: transparent;
  box-shadow: none;
  background-color: transparent;
}}
button.flat:hover, toolbar button:hover, .toolbar button:hover {{ border-color: {up}; }}
button.flat:active, button.flat:checked, toolbar button:active {{ border-color: {down}; }}
button.default, button.suggested-action {{ box-shadow: 0 0 0 2px {line}; }}
button.destructive-action {{ color: {text}; }}

/* Fields and lists: the well colour, sunken. */
entry, spinbutton:not(.vertical), textview, textview text, treeview.view,
list, listview, columnview, iconview, .view, scrolledwindow viewport,
flowbox, gridview {{
  background-color: {well};
  color: {text};
}}
entry, spinbutton:not(.vertical) {{
  border: 2px solid;
  border-color: {down};
  padding: 2px 4px;
  min-height: 18px;
}}
entry:focus, entry:focus-within {{ border-color: {down}; }}
entry:disabled {{ background-color: {face}; color: {dis}; }}
scrolledwindow, frame > border, .frame {{ border: 1px solid {line}; }}
scrolledwindow.frame {{ border-color: {line}; }}

selection, *:selected, *:selected:focus, .view:selected, row:selected,
treeview.view:selected, entry selection, textview text selection,
iconview:selected, flowboxchild:selected {{
  background-color: {sel};
  color: {sel_text};
}}

/* Menus: the face colour, the selection colour under the pointer. */
menubar, .menubar {{
  background-color: {face};
  color: {text};
  border-bottom: 1px solid {line};
  padding: 0;
}}
menu, .menu, .context-menu, popover.menu, popover.menu > contents {{
  background-color: {face};
  color: {text};
  border: 1px solid {line};
  padding: 1px;
}}
menuitem, modelbutton {{ padding: 3px 14px; }}
menubar > menuitem {{ padding: 2px 8px; }}
menuitem:hover, menubar > menuitem:hover, modelbutton:hover,
popover.menu modelbutton:hover {{
  background-color: {sel};
  color: {sel_text};
}}
menuitem:disabled, modelbutton:disabled {{ color: {dis}; }}
separator {{ background-color: {line}; min-width: 1px; min-height: 1px; }}

/* Title bars a program draws itself (GTK header bars). */
headerbar {{
  background-color: {active};
  color: {active_text};
  border: none;
  border-bottom: 1px solid {line};
  min-height: 24px;
  padding: 0 4px;
}}
headerbar:backdrop {{ background-color: {inactive}; color: {inactive_text}; }}
headerbar .title {{ font-weight: bold; color: {active_text}; }}
headerbar:backdrop .title {{ color: {inactive_text}; }}
headerbar button {{ min-height: 16px; padding: 1px 6px; }}

/* Notebook tabs. */
notebook > header {{ background-color: {face}; border: none; }}
notebook > header tab {{ margin: 2px 1px 0 1px; padding: 2px 10px; box-shadow: none; }}
notebook > header tab:checked {{ border-color: {up}; border-bottom-color: {face}; }}
notebook > stack:not(:only-child) {{ border: 2px solid; border-color: {up}; background-color: {face}; }}

/* Progress bars and sliders. */
progressbar trough, scale trough, levelbar trough {{
  background-color: {well};
  border: 2px solid;
  border-color: {down};
  min-height: 6px;
  min-width: 6px;
}}
progressbar progress, levelbar block.filled, scale highlight {{
  background-color: {sel};
  border: none;
  min-height: 6px;
}}
scale slider {{
  background-color: {face};
  border: 2px solid;
  border-color: {up};
  box-shadow: 0 0 0 1px {line};
  min-width: 10px;
  min-height: 18px;
  margin: -8px 0;
}}
scale.vertical slider {{ min-width: 18px; min-height: 10px; margin: 0 -8px; }}

/* Scroll bars: a trough of the face colour, a raised thumb. */
scrollbar {{
  background-color: {face};
  border: none;
}}
scrollbar slider {{
  background-color: {face};
  border: 2px solid;
  border-color: {up};
  box-shadow: 0 0 0 1px {line};
  min-width: 12px;
  min-height: 12px;
  margin: 1px;
}}
scrollbar button {{
  min-width: 12px;
  min-height: 12px;
  padding: 0;
  margin: 1px;
}}

/* Tool tips: the well colour, a black line. */
tooltip, tooltip.background {{
  background-color: {well};
  color: {text};
  border: 1px solid {line};
}}
tooltip * {{ color: {text}; }}

/* Switches look like a two-state button. */
switch {{
  background-color: {well};
  border: 2px solid;
  border-color: {down};
  min-width: 40px;
}}
switch:checked {{ background-color: {sel}; }}
switch slider {{
  background-color: {face};
  border: 2px solid;
  border-color: {up};
  min-width: 18px;
  min-height: 16px;
}}

link, *:link {{ color: {sel}; }}
"#,
        active = c.active,
        active_text = c.active_text,
        inactive = c.inactive,
        inactive_text = c.inactive_text,
    )
}

/// Check boxes and radio buttons from the PNGs written beside the CSS.
fn checks_css(c: &Colors, gtk4: bool) -> String {
    let img = |n: &str| {
        if gtk4 {
            format!("url(\"assets/{n}.png\")")
        } else {
            format!("-gtk-scaled(url(\"assets/{n}.png\"), url(\"assets/{n}@2.png\"))")
        }
    };
    format!(
        r#"
check, radio, checkbutton check, radiobutton radio, treeview.view check,
treeview.view radio {{
  min-width: 13px;
  min-height: 13px;
  margin: 0 4px 0 0;
  padding: 0;
  border: none;
  background-color: transparent;
  color: {text};
}}
check {{ -gtk-icon-source: {cb}; }}
check:checked {{ -gtk-icon-source: {cbx}; }}
check:indeterminate {{ -gtk-icon-source: {cbm}; }}
check:disabled {{ -gtk-icon-source: {cbd}; }}
check:checked:disabled {{ -gtk-icon-source: {cbxd}; }}
radio {{ -gtk-icon-source: {rb}; }}
radio:checked {{ -gtk-icon-source: {rbx}; }}
radio:disabled {{ -gtk-icon-source: {rbd}; }}
radio:checked:disabled {{ -gtk-icon-source: {rbxd}; }}
menuitem check, menuitem radio, modelbutton check, modelbutton radio {{ margin: 0 4px 0 -8px; }}

/* Arrows: solid triangles (no icon theme needed). */
arrow, combobox arrow, menuitem arrow, spinbutton button, expander {{
  min-width: 13px;
  min-height: 13px;
}}
arrow, combobox arrow, menubutton arrow, .down-arrow {{ -gtk-icon-source: {ad}; }}
arrow:disabled, combobox arrow:disabled {{ -gtk-icon-source: {add}; }}
arrow.up {{ -gtk-icon-source: {au}; }}
arrow.left {{ -gtk-icon-source: {al}; }}
arrow.right, menuitem arrow {{ -gtk-icon-source: {ar}; }}
scrollbar button.up, scrollbar button.top {{ -gtk-icon-source: {au}; }}
scrollbar button.down, scrollbar button.bottom {{ -gtk-icon-source: {ad}; }}
scrollbar button.left {{ -gtk-icon-source: {al}; }}
scrollbar button.right {{ -gtk-icon-source: {ar}; }}
scrollbar button {{
  border: 2px solid;
  border-color: {up};
  background-color: {face};
}}
scrollbar button:active {{ border-color: {down}; }}
scrollbar button:disabled {{ -gtk-icon-source: none; }}
expander {{ -gtk-icon-source: {ar}; }}
expander:checked {{ -gtk-icon-source: {ad}; }}
"#,
        text = c.text,
        cb = img("check"),
        cbx = img("check-checked"),
        cbm = img("check-mixed"),
        cbd = img("check-disabled"),
        cbxd = img("check-checked-disabled"),
        rb = img("radio"),
        rbx = img("radio-checked"),
        rbd = img("radio-disabled"),
        rbxd = img("radio-checked-disabled"),
        au = img("arrow-up"),
        ad = img("arrow-down"),
        al = img("arrow-left"),
        ar = img("arrow-right"),
        add = img("arrow-down-disabled"),
        up = raised(c),
        down = sunken(c),
        face = c.face,
    )
}

/// GTK 3 only: style properties (arrow buttons on scroll bars, no
/// overlay scroll bars) and widgets GTK 4 no longer has.
fn gtk3_css(c: &Colors) -> String {
    format!(
        r#"
* {{
  -GtkScrollbar-has-backward-stepper: true;
  -GtkScrollbar-has-forward-stepper: true;
  -GtkWidget-window-dragging: false;
  -GtkDialog-button-spacing: 8;
}}
scrollbar.overlay-indicator:not(.dragging):not(.hovering) slider {{
  min-width: 12px;
  min-height: 12px;
}}
.csd decoration, decoration {{
  border-radius: 0;
  box-shadow: 0 0 0 1px {line};
  margin: 0;
}}
toolbar, .inline-toolbar, .primary-toolbar {{
  background-color: {face};
  border-bottom: 1px solid {line};
  padding: 2px;
}}
infobar {{ border: none; }}
"#,
        line = c.line,
        face = c.face,
    )
}

/// A 13×13 check box (Windows 3.1: a white box, an X when set) or a
/// 12×12 radio button, as RGBA at `scale` (1 or 2).
fn picture(kind: &str, t: &Theme, scale: u32) -> (u32, Vec<u8>) {
    let n = 13u32;
    let disabled = kind.ends_with("-disabled");
    let ink = if disabled { t.disabled } else { t.text };
    let fill = if disabled { t.face } else { t.well };
    let mut px = vec![0u32; (n * n) as usize]; // 0 = clear
    if let Some(dir) = kind.strip_prefix("arrow-") {
        // A solid triangle, 7 wide and 4 deep, in the middle.
        let ink = if dir.ends_with("-disabled") {
            t.disabled
        } else {
            t.text
        };
        for i in 0..4i32 {
            for j in -(3 - i)..=(3 - i) {
                let (x, y) = match dir.trim_end_matches("-disabled") {
                    // Row i = 0 is the widest: the triangle's base.
                    "up" => (6 + j, 8 - i),
                    "down" => (6 + j, 5 + i),
                    "left" => (8 - i, 6 + j),
                    _ => (5 + i, 6 + j),
                };
                px[(y as u32 * n + x as u32) as usize] = 0xFF00_0000 | (ink & 0xFF_FFFF);
            }
        }
        return scaled(&px, n, scale);
    }
    let set = |px: &mut Vec<u32>, x: i32, y: i32, c: Rgb| {
        if (0..n as i32).contains(&x) && (0..n as i32).contains(&y) {
            px[(y as u32 * n + x as u32) as usize] = 0xFF00_0000 | (c & 0xFF_FFFF);
        }
    };
    if kind.starts_with("check") {
        for y in 0..n as i32 {
            for x in 0..n as i32 {
                let edge = x == 0 || y == 0 || x == n as i32 - 1 || y == n as i32 - 1;
                set(&mut px, x, y, if edge { t.line } else { fill });
            }
        }
        if kind.contains("checked") {
            for i in 2..n as i32 - 2 {
                set(&mut px, i, i, ink);
                set(&mut px, n as i32 - 1 - i, i, ink);
            }
        } else if kind.contains("mixed") {
            for x in 3..n as i32 - 3 {
                set(&mut px, x, 6, ink);
                set(&mut px, x, 7, ink);
            }
        }
    } else {
        // A pixel circle: outline, filled, a dot when set.
        let (cx, cy) = (6.0f32, 6.0f32);
        for y in 0..n as i32 {
            for x in 0..n as i32 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                if d <= 5.6 {
                    set(&mut px, x, y, if d > 4.6 { t.line } else { fill });
                }
                if kind.contains("checked") && d <= 2.3 {
                    set(&mut px, x, y, ink);
                }
            }
        }
    }
    scaled(&px, n, scale)
}

/// Scale an n×n picture up by pixel replication, as RGBA bytes.
fn scaled(px: &[u32], n: u32, scale: u32) -> (u32, Vec<u8>) {
    let w = n * scale;
    let mut out = Vec::with_capacity((w * w * 4) as usize);
    for y in 0..w {
        for x in 0..w {
            let v = px[((y / scale) * n + x / scale) as usize];
            out.extend_from_slice(&[(v >> 16) as u8, (v >> 8) as u8, v as u8, (v >> 24) as u8]);
        }
    }
    (w, out)
}

fn write_png(path: &Path, w: u32, rgba: &[u8]) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, w);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)?;
    Ok(())
}

const PICTURES: [&str; 17] = [
    "arrow-up",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "arrow-up-disabled",
    "arrow-down-disabled",
    "arrow-left-disabled",
    "arrow-right-disabled",
    "check",
    "check-checked",
    "check-mixed",
    "check-disabled",
    "check-checked-disabled",
    "radio",
    "radio-checked",
    "radio-disabled",
    "radio-checked-disabled",
];

/// The whole CSS for one scheme and GTK version.
pub fn css(scheme: &Scheme, gtk4: bool) -> String {
    let t = Theme::for_scheme(scheme);
    let c = Colors::of(&t);
    let mut s = format!(
        "/* Phosphor ({}) for GTK {} — written by tools/retro-gtktheme from\n   retro_gfx::Theme (D-056); do not edit. */\n",
        scheme.name,
        if gtk4 { 4 } else { 3 }
    );
    s.push_str(&named_colors(&c));
    s.push_str(&common_css(&c));
    s.push_str(&checks_css(&c, gtk4));
    // The dotted focus line: GTK 3 draws an outline only on the focused
    // widget, GTK 4 wherever one is set.
    let focus = if gtk4 { "*:focus-visible" } else { "*" };
    s.push_str(&format!(
        "\n{focus} {{\n  outline-color: {};\n  outline-style: dotted;\n  outline-width: 1px;\n  outline-offset: -3px;\n}}\n",
        c.text
    ));
    if !gtk4 {
        s.push_str(&gtk3_css(&c));
    }
    s
}

fn write_theme(dir: &Path, scheme: &Scheme, name: &str) -> std::io::Result<usize> {
    let t = Theme::for_scheme(scheme);
    let mut n = 0;
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("index.theme"),
        format!(
            "[Desktop Entry]\nType=X-GNOME-Metatheme\nName={name}\nComment=RetroOS {} colours for GTK programs\nEncoding=UTF-8\n\n[X-GNOME-Metatheme]\nGtkTheme={name}\n",
            scheme.name
        ),
    )?;
    for (sub, gtk4) in [("gtk-3.0", false), ("gtk-4.0", true)] {
        let d = dir.join(sub);
        let assets = d.join("assets");
        std::fs::create_dir_all(&assets)?;
        std::fs::write(d.join("gtk.css"), css(scheme, gtk4))?;
        n += 1;
        for p in PICTURES {
            let (w, px) = picture(p, &t, 1);
            write_png(&assets.join(format!("{p}.png")), w, &px)?;
            let (w, px) = picture(p, &t, 2);
            write_png(&assets.join(format!("{p}@2.png")), w, &px)?;
            n += 2;
        }
    }
    Ok(n)
}

fn main() {
    let Some(root) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: retro-gtktheme THEMES_DIR");
        std::process::exit(2);
    };
    let mut n = 0;
    for s in Scheme::ALL {
        let name = format!("Phosphor-{}", s.key());
        match write_theme(&root.join(&name), s, &name) {
            Ok(k) => n += k,
            Err(e) => {
                eprintln!("retro-gtktheme: {name}: {e}");
                std::process::exit(1);
            }
        }
    }
    // "Phosphor": the Classic colours, for anything that asks by that name.
    if let Err(e) = write_theme(&root.join("Phosphor"), &Scheme::CLASSIC, "Phosphor") {
        eprintln!("retro-gtktheme: Phosphor: {e}");
        std::process::exit(1);
    }
    println!("retro-gtktheme: {} files in {}", n, root.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scheme_has_its_colours() {
        for s in Scheme::ALL {
            let t = Theme::for_scheme(s);
            let c = css(s, false);
            assert!(c.contains(&format!("@define-color theme_bg_color {};", hex(t.face))));
            assert!(c.contains(&format!(
                "@define-color theme_selected_bg_color {};",
                hex(t.sel)
            )));
            assert!(c.contains("-GtkScrollbar-has-backward-stepper"));
            assert!(
                !css(s, true).contains("-GtkScrollbar"),
                "GTK 4 has no style properties"
            );
            // Braces balance (a format! slip would break the whole file).
            assert_eq!(c.matches('{').count(), c.matches('}').count());
        }
    }

    #[test]
    fn arrows_point_the_right_way() {
        let t = Theme::for_scheme(&Scheme::CLASSIC);
        let (w, px) = picture("arrow-down", &t, 1);
        let opaque = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize] != 0;
        // Down: the base (7 wide) on top, the point below.
        assert!(opaque(3, 5) && opaque(9, 5) && !opaque(3, 8) && opaque(6, 8));
    }

    #[test]
    fn a_checked_box_has_an_x() {
        let t = Theme::for_scheme(&Scheme::CLASSIC);
        let (w, px) = picture("check-checked", &t, 1);
        assert_eq!(w, 13);
        let at = |x: u32, y: u32| &px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 3) as usize];
        let ink = [(t.text >> 16) as u8, (t.text >> 8) as u8, t.text as u8];
        assert_eq!(at(3, 3), ink);
        assert_eq!(at(9, 3), ink);
        let (w2, px2) = picture("radio", &t, 2);
        assert_eq!(w2, 26);
        assert_eq!(px2[3], 0, "a radio button's corner is clear");
    }
}
