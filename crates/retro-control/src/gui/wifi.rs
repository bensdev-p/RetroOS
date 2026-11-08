//! Control Panel → Network → Wireless networks: the Desktop's counterpart of
//! NET's Wi-Fi list (retro_sys::net, iwd). Networks in range with their
//! signal, security and state; Connect asks for what the network needs (a
//! passphrase, or a user name and password for enterprise 802.1X sign-in,
//! D-045); Other... joins a hidden network. Slow calls run from a timer
//! after the dialog has drawn "Connecting...", as NET draws its busy box.

use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{bevel, ellipsize_in, push_button};
use retro_gfx::{Canvas, Surface, Theme};
use retro_sys::net::{self, Network, Security, WifiApi};
use retro_sys::netconf;
use retro_ui::controls::{TextField, FIELD_H, ROW_H};
use retro_ui::keys::*;
use retro_ui::{Event, Rect, SurfaceId, Ui};

pub const W: i32 = 420;
const LIST: Rect = Rect {
    x: 14,
    y: 34,
    w: W - 28,
    h: 4 + 10 * ROW_H,
};
const H: i32 = LIST.y + LIST.h + 52;
const BTN_W: i32 = 72;
const BTN_H: i32 = 24;
const BUTTONS: [&str; 5] = ["Connect", "Disconnect", "Forget", "Other...", "Close"];
const LEFT_BUTTON: u32 = 0x110;

/// Timer tokens (the Control Panel routes them here).
pub const TIMER_JOB: u32 = 20;
pub const TIMER_REFRESH: u32 = 21;

/// A slow call, run after "busy" has been drawn.
pub enum Job {
    Connect(Network, Option<String>),
    Hidden(String, Option<String>),
    /// Teach iwd the sign-in, then connect.
    Enterprise(Network, String, String, String),
    Disconnect,
    Forget(Network),
}

impl Job {
    fn busy(&self) -> String {
        match self {
            Job::Connect(n, _) | Job::Enterprise(n, ..) => format!("Connecting to {}...", n.ssid),
            Job::Hidden(s, _) => format!("Connecting to {s}..."),
            Job::Disconnect => "Disconnecting...".into(),
            Job::Forget(n) => format!("Forgetting {}...", n.ssid),
        }
    }
}

/// What the sign-in box asks for.
enum AskKind {
    Passphrase(Network),
    Enterprise(Network),
    Hidden,
}

/// The sign-in box over the dialog.
pub struct Ask {
    pub surface: SurfaceId,
    kind: AskKind,
    /// EAP method index (enterprise only).
    method: usize,
    fields: Vec<(&'static str, TextField)>,
    /// 0 = the method (enterprise), then the fields, then Connect, Cancel.
    focus: usize,
    pressed: Option<usize>,
    h: i32,
}

const ASK_W: i32 = 330;
const ASK_X: i32 = 120;

impl Ask {
    fn first(&self) -> usize {
        usize::from(!matches!(self.kind, AskKind::Enterprise(_)))
    }
    fn n(&self) -> usize {
        1 + self.fields.len()
    }
    fn row_y(&self, i: usize) -> i32 {
        40 + i as i32 * 28
    }
    fn button(&self, b: usize) -> Rect {
        Rect::new(
            ASK_W - 2 * (76 + 10) - 4 + b as i32 * 86,
            self.h - BTN_H - 12,
            76,
            BTN_H,
        )
    }
    fn title(&self) -> String {
        match &self.kind {
            AskKind::Passphrase(n) => format!("{} needs a passphrase.", n.ssid),
            AskKind::Enterprise(n) => format!("{} needs your sign-in.", n.ssid),
            AskKind::Hidden => "Join a network that does not show its name.".into(),
        }
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        sf.text(
            font,
            14,
            12,
            1,
            t.text,
            None,
            &ellipsize_in(font, &self.title(), ASK_W - 28),
        );
        if let AskKind::Enterprise(_) = self.kind {
            let y = self.row_y(0);
            sf.text(font, 14, y + 4, 1, t.text, None, "Sign-in method:");
            let (_, label) = netconf::EAP_METHODS[self.method];
            choice_box(&mut sf, ASK_X, y, 120, label, self.focus == 0, t);
        }
        for (i, (label, f)) in self.fields.iter().enumerate() {
            let y = self.row_y(i + 1);
            sf.text(font, 14, y + 4, 1, t.text, None, &format!("{label}:"));
            f.draw(
                &mut sf,
                ASK_X,
                y,
                ASK_W - ASK_X - 14,
                self.focus == i + 1,
                t,
            );
        }
        for (b, label) in ["Connect", "Cancel"].iter().enumerate() {
            let r = self.button(b);
            let default = self.focus == self.n() + b || (b == 0 && self.focus < self.n());
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                label,
                default,
                self.pressed == Some(b),
                t,
            );
        }
    }
}

/// A small drop-down-looking box (the method, which a click steps).
fn choice_box(sf: &mut Surface, x: i32, y: i32, w: i32, text: &str, focused: bool, t: &Theme) {
    sf.fill_rect(x, y, w as u32, FIELD_H as u32, t.well);
    sf.frame(x, y, w as u32, FIELD_H as u32, 1, t.line);
    let font = lantern_12();
    let fg = if focused {
        sf.fill_rect(
            x + 2,
            y + 2,
            (font.text_width(text) as i32 + 4) as u32,
            16,
            t.sel,
        );
        t.sel_text
    } else {
        t.text
    };
    sf.text(font, x + 4, y + 4, 1, fg, None, text);
    let bx = x + w - 18;
    bevel(sf, bx, y + 1, 17, FIELD_H - 2, 1, false, t);
    for i in 0..4 {
        sf.hline(bx + 5 + i, y + 8 + i, (7 - 2 * i) as u32, t.text);
    }
}

/// What the dialog asks the Control Panel to do after an event.
#[derive(Debug, PartialEq)]
pub enum Out {
    None,
    Close,
    /// Show a message box with these lines.
    Message(Vec<String>),
}

pub struct WifiDlg {
    pub surface: SurfaceId,
    pub ask: Option<Ask>,
    wifi: Option<Box<dyn WifiApi>>,
    why: Option<String>,
    nets: Vec<Network>,
    sel: usize,
    top: usize,
    /// 0 = the list, 1… = the buttons.
    focus: usize,
    pressed: Option<usize>,
    busy: Option<String>,
    pending: Option<Job>,
}

fn open_wifi() -> (Option<Box<dyn WifiApi>>, Option<String>) {
    if std::env::var_os("RETROOS_TEST_WIFI").is_some() {
        return (Some(Box::new(fake::Fake::default())), None);
    }
    match net::Wifi::open() {
        Ok(Some(w)) => (Some(Box::new(w)), None),
        Ok(None) => (None, Some("There is no wireless adapter.".into())),
        Err(e) if e.contains("not available") || e.contains("ServiceUnknown") => (
            None,
            Some("The wireless service (iwd) is not running.".into()),
        ),
        Err(e) => (None, Some(e)),
    }
}

/// Four rising bars, `n` of them filled.
fn bars(sf: &mut Surface, x: i32, y: i32, n: u8, fg: retro_gfx::Rgb, dim: retro_gfx::Rgb) {
    for i in 0..4 {
        let h = 3 + i * 3;
        let c = if (i as u8) < n { fg } else { dim };
        sf.fill_rect(x + i * 4, y + 12 - h, 3, h as u32, c);
    }
}

impl WifiDlg {
    pub fn new(ui: &mut Ui, parent: SurfaceId) -> WifiDlg {
        let (wifi, why) = open_wifi();
        if let Some(w) = &wifi {
            let _ = w.scan();
        }
        let surface = ui.dialog(parent, "Wireless Networks", (W, H));
        let mut d = WifiDlg {
            surface,
            ask: None,
            wifi,
            why,
            nets: Vec::new(),
            sel: 0,
            top: 0,
            focus: 0,
            pressed: None,
            busy: None,
            pending: None,
        };
        d.refresh();
        ui.timer(TIMER_REFRESH, std::time::Duration::from_secs(3));
        d
    }

    fn rows() -> usize {
        ((LIST.h - 4) / ROW_H) as usize
    }

    /// Re-read the networks, keeping the selection on the same one.
    pub fn refresh(&mut self) {
        let Some(w) = &self.wifi else { return };
        if let Ok(n) = w.networks() {
            let keep = self.nets.get(self.sel).map(|n| n.ssid.clone());
            self.nets = n;
            if let Some(i) = keep.and_then(|k| self.nets.iter().position(|n| n.ssid == k)) {
                self.sel = i;
            }
            self.sel = self.sel.min(self.nets.len().saturating_sub(1));
            self.reveal();
        }
    }

    fn reveal(&mut self) {
        let rows = WifiDlg::rows();
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + rows {
            self.top = self.sel + 1 - rows;
        }
    }

    fn status(&self) -> String {
        if let Some(b) = &self.busy {
            return b.clone();
        }
        if let Some(w) = &self.why {
            return w.clone();
        }
        let Some(w) = &self.wifi else {
            return String::new();
        };
        match self.nets.iter().find(|n| n.connected) {
            Some(n) => format!("Connected to {} ({}).", n.ssid, w.device()),
            None if w.scanning() => "Looking for networks...".into(),
            None if self.nets.is_empty() => "No networks in range.".into(),
            None => "Not connected. Choose a network and Connect.".into(),
        }
    }

    fn button(b: usize) -> Rect {
        let x = LIST.x + b as i32 * (BTN_W + 8);
        Rect::new(x, H - BTN_H - 12, BTN_W, BTN_H)
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        sf.text(
            font,
            LIST.x,
            12,
            1,
            t.text,
            None,
            &ellipsize_in(font, &self.status(), W - 28),
        );
        let r = LIST;
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.well);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
        for (row, i) in (self.top..self.nets.len())
            .take(WifiDlg::rows())
            .enumerate()
        {
            let n = &self.nets[i];
            let y = r.y + 2 + row as i32 * ROW_H;
            let (fg, dim) = if i == self.sel {
                let bg = if self.focus == 0 { t.sel } else { t.dark };
                sf.fill_rect(r.x + 2, y, (r.w - 4) as u32, ROW_H as u32, bg);
                (t.sel_text, t.well)
            } else {
                (t.text, t.disabled)
            };
            bars(&mut sf, r.x + 6, y + 2, n.bars(), fg, dim);
            sf.text(
                font,
                r.x + 28,
                y + 2,
                1,
                fg,
                None,
                &ellipsize_in(font, &n.ssid, 190),
            );
            sf.text(font, r.x + 226, y + 2, 1, fg, None, n.security.label());
            let state = if n.connected {
                "Connected"
            } else if n.known {
                "Saved"
            } else {
                ""
            };
            sf.text(font, r.x + 300, y + 2, 1, fg, None, state);
        }
        for (b, label) in BUTTONS.iter().enumerate() {
            let r = WifiDlg::button(b);
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                label,
                self.focus == b + 1 || (b == 0 && self.focus == 0),
                self.pressed == Some(b),
                t,
            );
        }
    }

    /// Start `job`: show its busy text now, run it from the timer.
    fn start(&mut self, ui: &mut Ui, job: Job) {
        self.busy = Some(job.busy());
        self.pending = Some(job);
        ui.redraw(self.surface);
        ui.timer(TIMER_JOB, std::time::Duration::from_millis(40));
    }

    /// The timer: run the pending job, or refresh the list.
    pub fn timer(&mut self, ui: &mut Ui, token: u32) -> Out {
        let mut out = Out::None;
        if token == TIMER_JOB {
            if let (Some(job), Some(w)) = (self.pending.take(), &self.wifi) {
                let r = match job {
                    Job::Connect(n, p) => w.connect(&n, p),
                    Job::Hidden(s, p) => w.connect_hidden(&s, p),
                    Job::Enterprise(n, method, user, pass) => {
                        netconf::provision_enterprise(&n.ssid, &method, &user, &pass)
                            .and_then(|()| w.connect(&n, None))
                    }
                    Job::Disconnect => w.disconnect(),
                    Job::Forget(n) => w.forget(&n),
                };
                if let Err(e) = r {
                    out = Out::Message(vec![e]);
                }
            }
            self.busy = None;
        } else if token == TIMER_REFRESH {
            ui.timer(TIMER_REFRESH, std::time::Duration::from_secs(3));
        }
        self.refresh();
        ui.redraw(self.surface);
        out
    }

    fn selected(&self) -> Option<Network> {
        self.nets.get(self.sel).cloned()
    }

    fn ask(&mut self, ui: &mut Ui, kind: AskKind) {
        let mut fields = Vec::new();
        let mut method = 0;
        match &kind {
            AskKind::Passphrase(_) => fields.push(("Passphrase", TextField::password())),
            AskKind::Enterprise(_) => {
                method = 0;
                fields.push(("User name", TextField::default()));
                fields.push(("Password", TextField::password()));
            }
            AskKind::Hidden => {
                fields.push(("Network name", TextField::default()));
                fields.push(("Passphrase", TextField::password()));
            }
        }
        let rows = fields.len() + usize::from(matches!(kind, AskKind::Enterprise(_)));
        let h = 40 + rows as i32 * 28 + 12 + BTN_H + 12;
        let title = match &kind {
            AskKind::Hidden => "Other Network",
            _ => "Connect",
        };
        let surface = ui.dialog(self.surface, title, (ASK_W, h));
        let mut a = Ask {
            surface,
            kind,
            method,
            fields,
            focus: 0,
            pressed: None,
            h,
        };
        // Start in the first text field (the method has a sensible default).
        a.focus = 1;
        self.ask = Some(a);
    }

    fn close_ask(&mut self, ui: &mut Ui) {
        if let Some(a) = self.ask.take() {
            ui.close(a.surface);
        }
    }

    /// Connect to the selected network, asking first when it needs it.
    fn connect(&mut self, ui: &mut Ui) -> Out {
        let Some(n) = self.selected() else {
            return Out::None;
        };
        if n.connected {
            return Out::Message(vec![format!("You are connected to {}.", n.ssid)]);
        }
        match n.security {
            Security::Open => self.start(ui, Job::Connect(n, None)),
            _ if n.known => self.start(ui, Job::Connect(n, None)),
            Security::Psk => self.ask(ui, AskKind::Passphrase(n)),
            Security::Enterprise => self.ask(ui, AskKind::Enterprise(n)),
            Security::Other => {
                return Out::Message(vec![format!(
                    "{} uses a kind of security RetroOS cannot join yet.",
                    n.ssid
                )])
            }
        }
        Out::None
    }

    fn press(&mut self, ui: &mut Ui, b: usize) -> Out {
        if BUTTONS[b] == "Close" {
            return Out::Close;
        }
        if self.wifi.is_none() {
            return Out::Message(vec![self
                .why
                .clone()
                .unwrap_or_else(|| "There is no wireless adapter.".into())]);
        }
        if self.busy.is_some() {
            return Out::None;
        }
        match BUTTONS[b] {
            "Connect" => return self.connect(ui),
            "Disconnect" => self.start(ui, Job::Disconnect),
            "Forget" => match self.selected() {
                Some(n) if n.known => self.start(ui, Job::Forget(n)),
                Some(n) => return Out::Message(vec![format!("{} is not remembered.", n.ssid)]),
                None => {}
            },
            _ => self.ask(ui, AskKind::Hidden),
        }
        Out::None
    }

    pub fn event(&mut self, ui: &mut Ui, ev: Event) -> Out {
        match ev {
            Event::Close => return Out::Close,
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    let hit = self
                        .pressed
                        .take()
                        .filter(|b| WifiDlg::button(*b).contains(x, y));
                    ui.redraw(self.surface);
                    return match hit {
                        Some(b) => self.press(ui, b),
                        None => Out::None,
                    };
                }
                if let Some(b) = (0..BUTTONS.len()).find(|b| WifiDlg::button(*b).contains(x, y)) {
                    self.pressed = Some(b);
                    self.focus = b + 1;
                } else if LIST.contains(x, y) {
                    let i = self.top + ((y - LIST.y - 2).max(0) / ROW_H) as usize;
                    self.focus = 0;
                    if i < self.nets.len() {
                        let again = i == self.sel;
                        self.sel = i;
                        if again {
                            // A second click on the same network connects.
                            ui.redraw(self.surface);
                            return self.connect(ui);
                        }
                    }
                }
            }
            Event::Scroll { steps } => {
                let max = self.nets.len().saturating_sub(WifiDlg::rows());
                self.top =
                    (self.top as isize + steps.round() as isize).clamp(0, max as isize) as usize;
            }
            Event::Key { sym, .. } => {
                let nb = BUTTONS.len();
                match sym {
                    ESCAPE => return Out::Close,
                    TAB => self.focus = (self.focus + 1) % (nb + 1),
                    ISO_LEFT_TAB => self.focus = (self.focus + nb) % (nb + 1),
                    UP if !self.nets.is_empty() => {
                        self.focus = 0;
                        self.sel = self.sel.saturating_sub(1);
                        self.reveal();
                    }
                    DOWN if !self.nets.is_empty() => {
                        self.focus = 0;
                        self.sel = (self.sel + 1).min(self.nets.len() - 1);
                        self.reveal();
                    }
                    LEFT if self.focus > 1 => self.focus -= 1,
                    RIGHT if self.focus >= 1 && self.focus < nb => self.focus += 1,
                    RETURN | KP_ENTER | SPACE => {
                        let b = self.focus.saturating_sub(1);
                        return self.press(ui, b);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        ui.redraw(self.surface);
        Out::None
    }

    /// Connect from the sign-in box.
    fn submit(&mut self, ui: &mut Ui) -> Out {
        // Check first: a wrong entry keeps the box open under the message.
        if let Some(a) = &self.ask {
            let v = |i: usize| a.fields[i].1.value();
            let bad = match a.kind {
                AskKind::Passphrase(_) => {
                    let n = v(0).chars().count();
                    (!(8..=63).contains(&n)).then_some("A passphrase has 8 to 63 characters.")
                }
                AskKind::Enterprise(_) => (v(0).trim().is_empty() || v(1).is_empty())
                    .then_some("Type your user name and password."),
                AskKind::Hidden => {
                    let n = v(1).chars().count();
                    if v(0).trim().is_empty() {
                        Some("Type the network's name.")
                    } else if n > 0 && !(8..=63).contains(&n) {
                        Some("A passphrase has 8 to 63 characters.")
                    } else {
                        None
                    }
                }
            };
            if let Some(m) = bad {
                return Out::Message(vec![m.into()]);
            }
        }
        let Some(a) = self.ask.take() else {
            return Out::None;
        };
        ui.close(a.surface);
        let v = |i: usize| a.fields[i].1.value();
        let pass = |s: String| (!s.is_empty()).then_some(s);
        let job = match a.kind {
            AskKind::Passphrase(n) => Job::Connect(n, Some(v(0))),
            AskKind::Enterprise(n) => {
                let method = netconf::EAP_METHODS[a.method].0.to_string();
                Job::Enterprise(n, method, v(0).trim().to_string(), v(1))
            }
            AskKind::Hidden => Job::Hidden(v(0).trim().to_string(), pass(v(1))),
        };
        self.start(ui, job);
        Out::None
    }

    pub fn ask_event(&mut self, ui: &mut Ui, ev: Event) -> Out {
        let Some(a) = self.ask.as_mut() else {
            return Out::None;
        };
        let n = a.n();
        match ev {
            Event::Close => self.close_ask(ui),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    let hit = a.pressed.take().filter(|b| a.button(*b).contains(x, y));
                    ui.redraw(a.surface);
                    match hit {
                        Some(0) => return self.submit(ui),
                        Some(_) => self.close_ask(ui),
                        None => {}
                    }
                    return Out::None;
                }
                if let Some(b) = (0..2).find(|b| a.button(*b).contains(x, y)) {
                    a.pressed = Some(b);
                } else if let Some(i) = (a.first()..n)
                    .find(|i| y >= a.row_y(*i) && y < a.row_y(*i) + FIELD_H && x >= ASK_X)
                {
                    a.focus = i;
                    if i == 0 {
                        a.method = (a.method + 1) % netconf::EAP_METHODS.len();
                    } else {
                        a.fields[i - 1].1.click(x - ASK_X);
                    }
                }
                ui.redraw(a.surface);
            }
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => {
                let first = a.first();
                let total = n + 2 - first;
                let at = a.focus - first;
                match sym {
                    ESCAPE => {
                        self.close_ask(ui);
                        return Out::None;
                    }
                    RETURN | KP_ENTER if a.focus == n + 1 => {
                        self.close_ask(ui);
                        return Out::None;
                    }
                    RETURN | KP_ENTER => return self.submit(ui),
                    TAB | DOWN => a.focus = first + (at + 1) % total,
                    ISO_LEFT_TAB | UP => a.focus = first + (at + total - 1) % total,
                    SPACE if a.focus == n => return self.submit(ui),
                    SPACE if a.focus == n + 1 => {
                        self.close_ask(ui);
                        return Out::None;
                    }
                    LEFT | RIGHT | SPACE if a.focus == 0 => {
                        a.method = (a.method + 1) % netconf::EAP_METHODS.len();
                    }
                    _ if a.focus >= 1 && a.focus < n => {
                        a.fields[a.focus - 1]
                            .1
                            .key(sym, text.as_deref(), ctrl, shift);
                    }
                    _ => {}
                }
                ui.redraw(a.surface);
            }
            _ => {}
        }
        Out::None
    }
}

/// A pretend adapter for host tests (`RETROOS_TEST_WIFI=1`): four networks,
/// connecting always works.
mod fake {
    use retro_sys::net::{Network, Security, WifiApi};
    use std::cell::RefCell;

    pub struct Fake {
        nets: RefCell<Vec<Network>>,
    }

    impl Default for Fake {
        fn default() -> Fake {
            let n = |ssid: &str, security, dbm, connected, known| Network {
                ssid: ssid.into(),
                path: ssid.into(),
                security,
                dbm,
                connected,
                known,
            };
            Fake {
                nets: RefCell::new(vec![
                    n("HomeNet", Security::Psk, -48, true, true),
                    n("eduroam", Security::Enterprise, -62, false, false),
                    n("Cafe Free WiFi", Security::Open, -70, false, false),
                    n("Neighbour", Security::Psk, -84, false, false),
                ]),
            }
        }
    }

    impl WifiApi for Fake {
        fn device(&self) -> String {
            "wlan0".into()
        }
        fn state(&self) -> String {
            "connected".into()
        }
        fn scanning(&self) -> bool {
            false
        }
        fn scan(&self) -> Result<(), String> {
            Ok(())
        }
        fn networks(&self) -> Result<Vec<Network>, String> {
            Ok(self.nets.borrow().clone())
        }
        fn connect(&self, net: &Network, _p: Option<String>) -> Result<(), String> {
            for n in self.nets.borrow_mut().iter_mut() {
                n.connected = n.ssid == net.ssid;
                if n.connected {
                    n.known = true;
                }
            }
            Ok(())
        }
        fn connect_hidden(&self, _ssid: &str, _p: Option<String>) -> Result<(), String> {
            Err("The network is out of range.".into())
        }
        fn disconnect(&self) -> Result<(), String> {
            for n in self.nets.borrow_mut().iter_mut() {
                n.connected = false;
            }
            Ok(())
        }
        fn forget(&self, net: &Network) -> Result<(), String> {
            for n in self.nets.borrow_mut().iter_mut() {
                if n.ssid == net.ssid {
                    n.known = false;
                    n.connected = false;
                }
            }
            Ok(())
        }
    }
}
