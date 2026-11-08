//! Control Panel (docs/DESIGN.md §8, §9.2): the applets and their settings
//! as plain data, shared by the text front-end (`CONTROL`, retro-control)
//! and the Desktop's Control Panel (control-panel), so both show and change
//! exactly the same things. Each applet [`load`]s a page of [`Field`]s;
//! the front-end lets the user edit them and calls [`apply`].

use retro_sys::{clock, display, keyboard, netconf, startup, users};

pub mod forms;
pub use forms::{action_label, Form};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applet {
    Display,
    Startup,
    DateTime,
    Keyboard,
    Network,
    Users,
}

pub const APPLETS: &[Applet] = &[
    Applet::Display,
    Applet::DateTime,
    Applet::Keyboard,
    Applet::Network,
    Applet::Startup,
    Applet::Users,
];

impl Applet {
    pub fn name(self) -> &'static str {
        match self {
            Applet::Display => "Display",
            Applet::Startup => "Startup",
            Applet::DateTime => "Date & Time",
            Applet::Keyboard => "Keyboard",
            Applet::Network => "Network",
            Applet::Users => "Users",
        }
    }

    /// The icon in `assets/icons` (retro_gfx::iconset::builtin).
    pub fn icon(self) -> &'static str {
        match self {
            Applet::Display => "cp-display",
            Applet::Startup => "cp-startup",
            Applet::DateTime => "cp-clock",
            Applet::Keyboard => "cp-keyboard",
            Applet::Network => "cp-network",
            Applet::Users => "cp-users",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Applet::Display => "Colours, screen rotation, size and brightness.",
            Applet::Startup => "The POST screen, the chime, and how RetroOS starts.",
            Applet::DateTime => "The date, the time and the time zone.",
            Applet::Keyboard => "The keyboard layout.",
            Applet::Network => "Wi-Fi networks, the wired address, the country.",
            Applet::Users => "Who can log on, their passwords and rights.",
        }
    }
}

/// A setting's value and how it is edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// One of a few: (value, label) and the chosen one.
    Choice(Vec<(String, String)>, usize),
    /// One of many (a scrolling list): (value, label) and the chosen one.
    List(Vec<(String, String)>, usize),
    Check(bool),
    /// 0–100 %.
    Level(u32),
    /// Typed text.
    Text(String),
    /// A button that opens more (the front-end knows what: [`ACTION_WIFI`]).
    Action(&'static str),
}

/// The Network page's "Wireless networks" button: NET in text mode, the
/// Wireless Networks dialog on the Desktop.
pub const ACTION_WIFI: &str = "wifi";

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub value: Value,
    pub enabled: bool,
}

impl Field {
    fn new(key: &'static str, label: &'static str, value: Value) -> Field {
        Field {
            key,
            label,
            value,
            enabled: true,
        }
    }

    /// The chosen value as text ("yes"/"no" for a check box).
    pub fn text(&self) -> String {
        match &self.value {
            Value::Choice(o, i) | Value::List(o, i) => {
                o.get(*i).map(|(v, _)| v.clone()).unwrap_or_default()
            }
            Value::Check(b) => if *b { "yes" } else { "no" }.into(),
            Value::Level(n) => n.to_string(),
            Value::Text(s) => s.clone(),
            Value::Action(_) => String::new(),
        }
    }

    /// The chosen value as the user sees it.
    pub fn shown(&self) -> String {
        match &self.value {
            Value::Choice(o, i) | Value::List(o, i) => {
                o.get(*i).map(|(_, l)| l.clone()).unwrap_or_default()
            }
            Value::Check(b) => if *b { "Yes" } else { "No" }.into(),
            Value::Level(n) => format!("{n} %"),
            Value::Text(s) => s.clone(),
            Value::Action(a) => action_label(a).into(),
        }
    }

    /// Next/previous choice, toggle, ±5 %: the arrow keys and clicks.
    pub fn step(&mut self, d: i32) {
        if !self.enabled {
            return;
        }
        match &mut self.value {
            Value::Choice(o, i) | Value::List(o, i) => {
                let n = o.len() as i32;
                if n > 0 {
                    *i = (*i as i32 + d).clamp(0, n - 1) as usize;
                }
            }
            Value::Check(b) => *b = !*b,
            Value::Level(n) => *n = (*n as i32 + d * 5).clamp(5, 100) as u32,
            Value::Text(_) | Value::Action(_) => {}
        }
    }

    /// Choice wrapping around (a click on a drop-down cycles).
    pub fn cycle(&mut self) {
        if let Value::Choice(o, i) = &mut self.value {
            if self.enabled && !o.is_empty() {
                *i = (*i + 1) % o.len();
            }
        } else {
            self.step(1);
        }
    }
}

/// An applet's settings, ready to edit.
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub applet: Applet,
    pub fields: Vec<Field>,
    /// Shown above the fields (a service that is missing, …).
    pub notice: Option<String>,
    /// The values as loaded, to apply only what changed.
    original: Vec<String>,
}

impl Page {
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.key == key)
    }

    pub fn changed(&self, key: &str) -> bool {
        self.fields
            .iter()
            .zip(&self.original)
            .any(|(f, o)| f.key == key && f.text() != *o)
    }

    pub fn any_changed(&self) -> bool {
        self.fields
            .iter()
            .zip(&self.original)
            .any(|(f, o)| f.text() != *o)
    }

    /// The page loaded again (after a form changed the system), keeping the
    /// chosen item of each list, or choosing `select` in it.
    pub fn reloaded(&self, select: Option<&str>) -> Page {
        let mut p = load(self.applet);
        for f in p.fields.iter_mut() {
            let want = select
                .map(String::from)
                .or_else(|| self.field(f.key).map(Field::text));
            if let (Value::List(o, i), Some(w)) = (&mut f.value, want) {
                if let Some(j) = o.iter().position(|(v, _)| *v == w) {
                    *i = j;
                }
            }
        }
        p
    }

    /// Fields that depend on others: call after every change (the
    /// fixed-address fields only count when the address is fixed).
    pub fn update(&mut self) {
        if self.applet == Applet::Network {
            let fixed = self.field("wired").is_some_and(|f| f.text() == "static");
            for f in self.fields.iter_mut() {
                if matches!(f.key, "address" | "gateway" | "dns") {
                    f.enabled = fixed;
                }
            }
        }
    }
}

fn opts(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

fn choice(list: Vec<(String, String)>, current: &str) -> Value {
    let i = list.iter().position(|(v, _)| v == current).unwrap_or(0);
    Value::Choice(list, i)
}

fn yes(v: &str) -> bool {
    matches!(v, "yes" | "on" | "true" | "1")
}

/// Turns from the automatic picture (landscape, right way up), so the
/// same choice means the same on a monitor and on a portrait panel (D-044).
const ROTATIONS: &[(&str, &str)] = &[
    ("auto", "Normal (automatic)"),
    ("flip", "Upside down"),
    ("right", "Quarter turn right"),
    ("left", "Quarter turn left"),
];

/// A panel turn set by number (cmdline-style), shown so it is not lost.
fn rotations(current: &str) -> Vec<(String, String)> {
    let mut v = opts(ROTATIONS);
    if !ROTATIONS.iter().any(|(k, _)| *k == current) {
        v.push((current.to_string(), format!("Panel turned {current}\u{b0}")));
    }
    v
}

const SCALES: &[(&str, &str)] = &[
    ("auto", "Automatic"),
    ("1", "1x (small)"),
    ("2", "2x (large)"),
    ("3", "3x"),
    ("4", "4x"),
];

/// The session's colour scheme (Station's), else the saved one.
fn live_scheme(conf: &str) -> String {
    retro_ui::station::request(&["SCHEME"])
        .ok()
        .and_then(|(f, _)| f.into_iter().next())
        .filter(|k| display::SCHEMES.iter().any(|(s, _)| s == k))
        .unwrap_or_else(|| startup::get(conf, "scheme"))
}

/// The settings of `applet`, as the system has them now.
pub fn load(applet: Applet) -> Page {
    let conf = startup::read();
    let mut notice = None;
    let fields = match applet {
        Applet::Display => {
            let mut f = vec![
                Field::new(
                    "scheme",
                    "Colour scheme",
                    choice(opts(display::SCHEMES), &live_scheme(&conf)),
                ),
                Field::new("rotate", "Screen rotation", {
                    let cur = startup::get(&conf, "rotate");
                    choice(rotations(&cur), &cur)
                }),
                Field::new(
                    "ui_scale",
                    "Program size",
                    choice(opts(SCALES), &startup::get(&conf, "ui_scale")),
                ),
            ];
            match display::backlight() {
                Some(b) => f.push(Field::new(
                    "brightness",
                    "Brightness",
                    Value::Level(b.percent()),
                )),
                None => {
                    let mut b = Field::new("brightness", "Brightness", Value::Level(100));
                    b.enabled = false;
                    f.push(b);
                }
            }
            f
        }
        Applet::Startup => vec![
            Field::new(
                "mode",
                "Start in",
                choice(
                    opts(&[("text", "RetroDOS"), ("desktop", "RetroOS Desktop")]),
                    &startup::get(&conf, "mode"),
                ),
            ),
            Field::new(
                "fast_post",
                "Quick memory test",
                Value::Check(yes(&startup::get(&conf, "fast_post"))),
            ),
            Field::new(
                "chime",
                "Startup chime",
                Value::Check(yes(&startup::get(&conf, "chime"))),
            ),
            Field::new(
                "menu_timeout",
                "Startup Menu waits",
                choice(
                    opts(&[
                        ("5", "5 seconds"),
                        ("10", "10 seconds"),
                        ("30", "30 seconds"),
                        ("60", "1 minute"),
                        ("600", "10 minutes"),
                    ]),
                    &startup::get(&conf, "menu_timeout"),
                ),
            ),
        ],
        Applet::DateTime => {
            let (c, why) = clock::status_or_files();
            notice = why;
            let now = clock::now();
            let zones: Vec<(String, String)> = clock::timezones()
                .into_iter()
                .map(|z| (z.clone(), z.replace('_', " ")))
                .collect();
            let mut ntp = Field::new("ntp", "Set automatically", Value::Check(c.ntp));
            ntp.enabled = c.can_ntp;
            let mut date = Field::new(
                "date",
                "Date (mm-dd-yyyy)",
                Value::Text(clock::date_text(now)),
            );
            let mut time = Field::new("time", "Time (hh:mm)", Value::Text(clock::time_text(now)));
            date.enabled = !c.ntp && notice.is_none();
            time.enabled = date.enabled;
            let mut tz = Field::new("timezone", "Time zone", {
                // timedated says "Etc/UTC" where the list has "UTC".
                let tz = c.timezone.strip_prefix("Etc/").unwrap_or(&c.timezone);
                let i = zones
                    .iter()
                    .position(|(z, _)| *z == c.timezone || z == tz)
                    .unwrap_or(0);
                Value::List(zones, i)
            });
            tz.enabled = notice.is_none();
            vec![ntp, date, time, tz]
        }
        Applet::Keyboard => {
            let (layout, _variant) = keyboard::current();
            let list: Vec<(String, String)> = keyboard::layouts()
                .into_iter()
                .map(|(n, d)| {
                    let label = format!("{d} ({n})");
                    (n, label)
                })
                .collect();
            let i = list.iter().position(|(n, _)| *n == layout).unwrap_or(0);
            vec![Field::new("layout", "Layout", Value::List(list, i))]
        }
        Applet::Users => {
            let me = users::me();
            let list: Vec<(String, String)> = users::list()
                .into_iter()
                .map(|u| {
                    let mut label = u.label();
                    if u.admin {
                        label.push_str(" - administrator");
                    }
                    if u.name == me {
                        label.push_str(" (you)");
                    }
                    (u.name, label)
                })
                .collect();
            let i = list.iter().position(|(n, _)| *n == me).unwrap_or(0);
            vec![
                Field::new("users", "Users", Value::List(list, i)),
                Field::new("add", "New user", Value::Action(forms::ACTION_ADD_USER)),
                Field::new(
                    "password",
                    "Password",
                    Value::Action(forms::ACTION_PASSWORD),
                ),
                Field::new(
                    "admin",
                    "Administrator rights",
                    Value::Action(forms::ACTION_ADMIN),
                ),
                Field::new(
                    "remove",
                    "Remove user",
                    Value::Action(forms::ACTION_REMOVE_USER),
                ),
            ]
        }
        Applet::Network => {
            let cur = netconf::country();
            let mut list = vec![(
                String::new(),
                "Not set (world-wide channels only)".to_string(),
            )];
            list.extend(
                netconf::countries()
                    .into_iter()
                    .map(|(c, n)| (c.clone(), format!("{n} ({c})"))),
            );
            let i = list.iter().position(|(c, _)| *c == cur).unwrap_or(0);
            let w = netconf::wired();
            vec![
                Field::new("wifi", "Wireless networks", Value::Action(ACTION_WIFI)),
                Field::new(
                    "wired",
                    "Wired address",
                    choice(
                        opts(&[("dhcp", "Automatic (DHCP)"), ("static", "Fixed")]),
                        if w.dhcp { "dhcp" } else { "static" },
                    ),
                ),
                Field::new("address", "IP address/prefix", Value::Text(w.address)),
                Field::new("gateway", "Gateway", Value::Text(w.gateway)),
                Field::new("dns", "Name servers", Value::Text(w.dns)),
                Field::new("country", "Wi-Fi country", Value::List(list, i)),
            ]
        }
    };
    let original = fields.iter().map(Field::text).collect();
    let mut page = Page {
        applet,
        fields,
        notice,
        original,
    };
    page.update();
    page
}

/// What applying did, beyond "done".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    /// The new colour scheme, for the front-end to show at once.
    pub scheme: Option<String>,
    /// The keyboard layout changed (Station reloads it: broker KEYMAP).
    pub keymap: bool,
    /// Settings that take effect when the Desktop or RetroDOS next starts.
    pub restart: bool,
    /// Settings that take effect when the computer next starts.
    pub reboot: bool,
}

impl Applied {
    /// What to tell the user after saving, if anything.
    pub fn notice(&self) -> Option<[&'static str; 2]> {
        if self.reboot {
            Some([
                "The Wi-Fi country takes effect the",
                "next time the computer starts.",
            ])
        } else if self.restart {
            Some([
                "The new screen settings take effect the",
                "next time the Desktop or RetroDOS starts.",
            ])
        } else {
            None
        }
    }
}

/// Save what changed on `page`.
pub fn apply(page: &Page) -> Result<Applied, String> {
    let mut done = Applied::default();
    let val = |k: &str| page.field(k).map(Field::text).unwrap_or_default();
    match page.applet {
        Applet::Display | Applet::Startup => {
            let keys: &[&str] = if page.applet == Applet::Display {
                &["scheme", "rotate", "ui_scale"]
            } else {
                &["mode", "fast_post", "chime", "menu_timeout"]
            };
            let changes: Vec<(&str, String)> = keys
                .iter()
                .filter(|k| page.changed(k))
                .map(|k| (*k, val(k)))
                .collect();
            let pairs: Vec<(&str, &str)> = changes.iter().map(|(k, v)| (*k, v.as_str())).collect();
            startup::set(&pairs).map_err(|e| e.to_string())?;
            if page.changed("scheme") {
                done.scheme = Some(val("scheme"));
            }
            done.restart = page.changed("rotate") || page.changed("ui_scale");
            if page.changed("brightness") {
                if let Some(b) = display::backlight() {
                    let pct = val("brightness").parse().unwrap_or(100);
                    display::set_brightness(&b, pct)?;
                }
            }
        }
        Applet::DateTime => {
            if page.changed("timezone") {
                clock::set_timezone(&val("timezone"))?;
            }
            if page.changed("ntp") {
                clock::set_ntp(val("ntp") == "yes")?;
            }
            if val("ntp") != "yes" && (page.changed("date") || page.changed("time")) {
                let t = clock::parse_local(&val("date"), &val("time"))
                    .ok_or("Type the date as mm-dd-yyyy and the time as hh:mm.")?;
                clock::set_local(t)?;
            }
        }
        Applet::Keyboard => {
            if page.changed("layout") {
                keyboard::set(&val("layout"), "").map_err(|e| e.to_string())?;
                done.keymap = true;
            }
        }
        // Users change through forms, at once; the page itself saves nothing.
        Applet::Users => {}
        Applet::Network => {
            let wired = ["wired", "address", "gateway", "dns"];
            if wired.iter().any(|k| page.changed(k)) {
                netconf::set_wired(&netconf::Wired {
                    dhcp: val("wired") != "static",
                    address: val("address"),
                    gateway: val("gateway"),
                    dns: val("dns"),
                })?;
            }
            if page.changed("country") {
                netconf::set_country(&val("country")).map_err(|e| e.to_string())?;
                done.reboot = true;
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_step_and_cycle() {
        let mut f = Field::new(
            "rotate",
            "Screen rotation",
            choice(rotations("flip"), "flip"),
        );
        assert_eq!(f.shown(), "Upside down");
        f.step(1);
        f.step(1);
        assert_eq!(f.text(), "left", "stops at the end");
        f.cycle();
        assert_eq!(f.text(), "auto", "a click wraps around");
        let mut c = Field::new("chime", "Startup chime", Value::Check(true));
        c.step(-1);
        assert_eq!(c.text(), "no");
        let mut l = Field::new("brightness", "Brightness", Value::Level(7));
        l.step(-1);
        assert_eq!(l.text(), "5", "never fully dark");
        l.enabled = false;
        l.step(1);
        assert_eq!(l.text(), "5");
    }

    #[test]
    fn startup_page_saves_only_what_changed() {
        let dir = std::env::temp_dir().join(format!("retro-control-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conf = dir.join("startup.conf");
        std::fs::write(&conf, "mode=text   # text or desktop\nscheme=amber\n").unwrap();
        std::env::set_var("RETROOS_STARTUP_CONF", &conf);
        let mut p = load(Applet::Startup);
        assert_eq!(p.field("mode").unwrap().shown(), "RetroDOS");
        assert!(!p.any_changed());
        p.fields[0].step(1);
        p.fields[2].step(1);
        assert!(p.changed("mode") && !p.changed("fast_post"));
        let done = apply(&p).unwrap();
        assert!(!done.restart && done.scheme.is_none());
        let s = std::fs::read_to_string(&conf).unwrap();
        assert_eq!(
            s,
            "mode=desktop       # text or desktop\nscheme=amber\nchime=no\n"
        );
        let d = load(Applet::Display);
        assert_eq!(d.field("scheme").unwrap().shown(), "Amber");
        std::env::remove_var("RETROOS_STARTUP_CONF");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn fixed_address_fields_follow_the_choice() {
        let mut p = load(Applet::Network);
        assert_eq!(p.field("wifi").unwrap().value, Value::Action(ACTION_WIFI));
        let i = p.fields.iter().position(|f| f.key == "wired").unwrap();
        p.fields[i].value = choice(
            opts(&[("dhcp", "Automatic (DHCP)"), ("static", "Fixed")]),
            "dhcp",
        );
        p.update();
        assert!(!p.field("address").unwrap().enabled);
        p.fields[i].step(1);
        p.update();
        assert!(p.field("address").unwrap().enabled);
    }

    #[test]
    fn a_numeric_rotation_is_kept_visible() {
        let r = rotations("90");
        assert_eq!(r.last().unwrap().0, "90");
        assert_eq!(rotations("auto").len(), ROTATIONS.len());
    }
}

pub mod text;

/// The next applet after `from` whose name starts with `c` (a typed letter
/// cycles through Display and Date & Time).
pub fn applet_by_letter(from: usize, c: char) -> Option<usize> {
    let c = c.to_ascii_lowercase();
    let n = APPLETS.len();
    (1..=n)
        .map(|k| (from + k) % n)
        .find(|i| APPLETS[*i].name().to_ascii_lowercase().starts_with(c))
}

/// An applet by a name typed at the prompt (CONTROL DISPLAY, CONTROL TIME).
pub fn applet_named(name: &str) -> Option<Applet> {
    let n = name.to_ascii_lowercase();
    let n = n.trim_start_matches(['/', '-']);
    Some(match n {
        "display" | "screen" | "colour" | "color" => Applet::Display,
        "startup" | "start" | "boot" => Applet::Startup,
        "date" | "time" | "clock" | "datetime" => Applet::DateTime,
        "keyboard" | "keyb" => Applet::Keyboard,
        "network" | "net" | "wifi" => Applet::Network,
        "users" | "user" | "accounts" => Applet::Users,
        _ => return None,
    })
}
