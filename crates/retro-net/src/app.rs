//! The network setup screen (docs/STYLE.md §3 mock-up).

use retro_sys::net::{Link, Network, Security, WifiApi};
use retro_tui::{Field, Key, Screen, BLACK, BRICK, COBALT, LEMON, LIME, SILVER, TEAL, WHITE};

/// Slow system calls; the main loop shows `busy_text` while one runs.
pub enum Job {
    Connect(Network, Option<String>),
    Hidden(String, Option<String>),
    Disconnect,
    Forget(Network),
    Scan,
}

impl Job {
    pub fn busy_text(&self) -> String {
        match self {
            Job::Connect(n, _) => format!("Connecting to {}...", n.ssid),
            Job::Hidden(s, _) => format!("Connecting to {s}..."),
            Job::Disconnect => "Disconnecting...".into(),
            Job::Forget(n) => format!("Forgetting {}...", n.ssid),
            Job::Scan => "Looking for networks...".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    List,
    Button(usize),
}

const BUTTONS: &[&str] = &[
    "Join",
    "Disconnect",
    "Forget",
    "Hidden...",
    "Rescan",
    "Close",
];

pub enum Modal {
    None,
    /// Passphrase for a network (or a hidden one when `ssid` is a field).
    Join {
        net: Option<Network>,
        ssid: Field,
        pass: Field,
        focus: usize,
    },
    Message(String, Vec<String>),
}

pub struct App {
    pub wifi: Option<Box<dyn WifiApi>>,
    pub wifi_error: Option<String>,
    pub links: Vec<Link>,
    pub nets: Vec<Network>,
    pub cursor: usize,
    focus: Focus,
    pub modal: Modal,
    pub busy: Option<String>,
    pub quit: bool,
}

fn bars(n: u8) -> String {
    ['▂', '▄', '▆', '█']
        .iter()
        .enumerate()
        .map(|(i, c)| if (i as u8) < n { *c } else { '_' })
        .collect()
}

impl App {
    pub fn new(wifi: Option<Box<dyn WifiApi>>, wifi_error: Option<String>) -> App {
        App {
            wifi,
            wifi_error,
            links: Vec::new(),
            nets: Vec::new(),
            cursor: 0,
            focus: Focus::List,
            modal: Modal::None,
            busy: None,
            quit: false,
        }
    }

    /// Re-read state (called on a timer and after jobs).
    pub fn refresh(&mut self, links: Vec<Link>) {
        self.links = links;
        if let Some(w) = &self.wifi {
            if let Ok(n) = w.networks() {
                let keep = self.nets.get(self.cursor).map(|n| n.ssid.clone());
                self.nets = n;
                if let Some(k) = keep {
                    if let Some(i) = self.nets.iter().position(|n| n.ssid == k) {
                        self.cursor = i;
                    }
                }
                self.cursor = self.cursor.min(self.nets.len().saturating_sub(1));
            }
        }
    }

    fn message(&mut self, title: &str, lines: &[&str]) {
        self.modal = Modal::Message(title.into(), lines.iter().map(|s| s.to_string()).collect());
    }

    pub fn run(&mut self, job: Job) {
        let Some(w) = &self.wifi else { return };
        let r = match job {
            Job::Connect(n, p) => w.connect(&n, p),
            Job::Hidden(s, p) => w.connect_hidden(&s, p),
            Job::Disconnect => w.disconnect(),
            Job::Forget(n) => w.forget(&n),
            Job::Scan => w.scan(),
        };
        if let Err(e) = r {
            self.message("Network", &[&e]);
        }
    }

    fn join(&mut self) -> Option<Job> {
        let net = self.nets.get(self.cursor)?.clone();
        if net.connected {
            self.message("Network", &[&format!("You are connected to {}.", net.ssid)]);
            return None;
        }
        match net.security {
            Security::Open => Some(Job::Connect(net, None)),
            Security::Psk if net.known => Some(Job::Connect(net, None)),
            Security::Psk => {
                self.modal = Modal::Join {
                    net: Some(net),
                    ssid: Field::new(32, false),
                    pass: Field::new(63, true),
                    focus: 1,
                };
                None
            }
            _ => {
                self.message(
                    "Network",
                    &[
                        &format!("{} uses enterprise (802.1X) sign-in.", net.ssid),
                        "Set it up in the Control Panel.",
                    ],
                );
                None
            }
        }
    }

    /// Handle a key; a returned job must be run (it may take seconds).
    pub fn key(&mut self, k: Key) -> Option<Job> {
        match std::mem::replace(&mut self.modal, Modal::None) {
            Modal::Message(..) => return None,
            Modal::Join {
                net,
                mut ssid,
                mut pass,
                mut focus,
            } => {
                let fields = if net.is_some() { 1 } else { 2 };
                match k {
                    Key::Esc => return None,
                    Key::Tab | Key::Down | Key::Up | Key::BackTab if fields == 2 => {
                        focus = 1 - focus
                    }
                    Key::Enter => {
                        let p = (!pass.value.is_empty()).then(|| pass.value.clone());
                        if let Some(p) = &p {
                            if p.len() < 8 {
                                self.modal = Modal::Join {
                                    net,
                                    ssid,
                                    pass,
                                    focus,
                                };
                                return None;
                            }
                        }
                        return match net {
                            Some(n) => Some(Job::Connect(n, p)),
                            None if !ssid.value.trim().is_empty() => {
                                Some(Job::Hidden(ssid.value.trim().to_string(), p))
                            }
                            None => None,
                        };
                    }
                    k => {
                        if focus == 0 && fields == 2 {
                            ssid.edit(&k);
                        } else {
                            pass.edit(&k);
                        }
                    }
                }
                self.modal = Modal::Join {
                    net,
                    ssid,
                    pass,
                    focus,
                };
                return None;
            }
            Modal::None => {}
        }
        let nb = BUTTONS.len();
        match (k, self.focus) {
            (Key::Esc | Key::F(10), _) => self.quit = true,
            (Key::F(5), _) => return Some(Job::Scan),
            (Key::Tab, Focus::List) => self.focus = Focus::Button(0),
            (Key::Tab, Focus::Button(b)) if b + 1 < nb => self.focus = Focus::Button(b + 1),
            (Key::Tab, Focus::Button(_)) => self.focus = Focus::List,
            (Key::BackTab, Focus::List) => self.focus = Focus::Button(nb - 1),
            (Key::BackTab, Focus::Button(0)) => self.focus = Focus::List,
            (Key::BackTab, Focus::Button(b)) => self.focus = Focus::Button(b - 1),
            (Key::Left, Focus::Button(b)) => self.focus = Focus::Button((b + nb - 1) % nb),
            (Key::Right, Focus::Button(b)) => self.focus = Focus::Button((b + 1) % nb),
            (Key::Up, _) => {
                self.focus = Focus::List;
                self.cursor = self.cursor.saturating_sub(1);
            }
            (Key::Down, _) => {
                self.focus = Focus::List;
                self.cursor = (self.cursor + 1).min(self.nets.len().saturating_sub(1));
            }
            (Key::Enter, Focus::List) => return self.join(),
            (Key::Enter, Focus::Button(b)) => return self.button(b),
            (Key::Char(c), _) => {
                let c = c.to_ascii_lowercase();
                if let Some(b) = BUTTONS
                    .iter()
                    .position(|l| l.to_ascii_lowercase().starts_with(c))
                {
                    return self.button(b);
                }
            }
            _ => {}
        }
        None
    }

    fn button(&mut self, b: usize) -> Option<Job> {
        if self.wifi.is_none() && BUTTONS[b] != "Close" {
            self.message("Network", &["There is no wireless adapter."]);
            return None;
        }
        match BUTTONS[b] {
            "Join" => self.join(),
            "Disconnect" => Some(Job::Disconnect),
            "Forget" => match self.nets.get(self.cursor) {
                Some(n) if n.known => Some(Job::Forget(n.clone())),
                Some(n) => {
                    let msg = format!("{} is not remembered.", n.ssid);
                    self.message("Network", &[&msg]);
                    None
                }
                None => None,
            },
            "Hidden..." => {
                self.modal = Modal::Join {
                    net: None,
                    ssid: Field::new(32, false),
                    pass: Field::new(63, true),
                    focus: 0,
                };
                None
            }
            "Rescan" => Some(Job::Scan),
            _ => {
                self.quit = true;
                None
            }
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn draw(&self, s: &mut Screen) {
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Network Setup", BLACK, TEAL);
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        s.text(
            1,
            y,
            "Enter Join  Tab Buttons  F5 Rescan  Esc Close",
            BLACK,
            TEAL,
        );

        let w = s.cols.saturating_sub(4).min(100);
        let x = (s.cols - w) / 2;
        let h = s.rows.saturating_sub(4);
        s.dialog(x, 1, w, h, "Network", WHITE, COBALT);
        let inner = (w - 4) as usize;
        let mut row = 3u16;

        s.text(x + 2, row, "Wired", LEMON, COBALT);
        row += 1;
        let wired: Vec<&Link> = self.links.iter().filter(|l| l.kind == "ether").collect();
        if wired.is_empty() {
            s.text(x + 4, row, "No wired network adapter.", SILVER, COBALT);
            row += 1;
        }
        for l in wired {
            let addr = l.addresses.first().cloned().unwrap_or_default();
            let line = format!("{:<10} {:<30} {}", l.name, l.friendly_state(), addr);
            let fg = if l.state == "routable" { LIME } else { SILVER };
            s.text(x + 4, row, &clip(&line, inner - 2), fg, COBALT);
            row += 1;
        }
        row += 1;

        match &self.wifi {
            None => {
                s.text(x + 2, row, "Wireless", LEMON, COBALT);
                row += 1;
                let why = self
                    .wifi_error
                    .clone()
                    .unwrap_or_else(|| "No wireless adapter found.".into());
                s.text(x + 4, row, &clip(&why, inner - 2), SILVER, COBALT);
            }
            Some(wf) => {
                let state = wf.state();
                let head = format!(
                    "Wireless ({}): {}{}",
                    wf.device(),
                    match state.as_str() {
                        "connected" => "connected",
                        "connecting" => "connecting...",
                        "disconnected" => "not connected",
                        other => other,
                    },
                    if wf.scanning() { "   (scanning)" } else { "" }
                );
                s.text(x + 2, row, &clip(&head, inner), LEMON, COBALT);
                row += 1;
                let list_rows = (h as usize).saturating_sub(row as usize + 4).max(1);
                let top = self.cursor.saturating_sub(list_rows - 1);
                if self.nets.is_empty() {
                    s.text(x + 4, row, "No networks found yet.", SILVER, COBALT);
                }
                for (i, n) in self.nets.iter().enumerate().skip(top).take(list_rows) {
                    let status = if n.connected {
                        "Connected"
                    } else if n.known {
                        "Saved"
                    } else {
                        ""
                    };
                    let mark = if i == self.cursor { '►' } else { ' ' };
                    let line = format!(
                        "{mark} {:<24} {}  {:<7} {}",
                        clip(&n.ssid, 24),
                        bars(n.bars()),
                        n.security.label(),
                        status
                    );
                    let (fg, bg) = if i == self.cursor && self.focus == Focus::List {
                        (BLACK, TEAL)
                    } else if n.connected {
                        (LIME, COBALT)
                    } else {
                        (WHITE, COBALT)
                    };
                    s.fill(x + 3, row, w - 6, 1, ' ', fg, bg);
                    s.text(x + 3, row, &clip(&line, inner - 2), fg, bg);
                    row += 1;
                }
            }
        }

        // Buttons.
        let labels: Vec<String> = BUTTONS.iter().map(|b| format!("< {b} >")).collect();
        let total: usize = labels.iter().map(|l| l.chars().count() + 2).sum();
        let mut bx = x + (w.saturating_sub(total as u16)) / 2;
        let by = h - 1;
        for (i, l) in labels.iter().enumerate() {
            let (fg, bg) = if self.focus == Focus::Button(i) {
                (BLACK, TEAL)
            } else {
                (WHITE, COBALT)
            };
            s.text(bx, by, l, fg, bg);
            s.text(
                bx + 2,
                by,
                &BUTTONS[i][..1],
                if self.focus == Focus::Button(i) {
                    BLACK
                } else {
                    LEMON
                },
                bg,
            );
            bx += l.chars().count() as u16 + 2;
        }

        match &self.modal {
            Modal::Join {
                net,
                ssid,
                pass,
                focus,
            } => {
                let dw = 56u16.min(s.cols - 4);
                let dh = if net.is_some() { 8 } else { 10 };
                let dx = (s.cols - dw) / 2;
                let dy = (s.rows - dh) / 2;
                let title = match net {
                    Some(n) => format!("Join {}", n.ssid),
                    None => "Join a Hidden Network".into(),
                };
                s.dialog(dx, dy, dw, dh, &title, BLACK, SILVER);
                let mut r = dy + 2;
                if net.is_none() {
                    s.text(dx + 3, r, "Network name:", BLACK, SILVER);
                    let bg = if *focus == 0 { TEAL } else { COBALT };
                    let cx = ssid.draw(s, dx + 18, r, dw - 21, WHITE, bg);
                    if *focus == 0 {
                        s.cursor = Some((cx, r));
                    }
                    r += 2;
                }
                s.text(dx + 3, r, "Passphrase:", BLACK, SILVER);
                let bg = if *focus == 1 || net.is_some() {
                    TEAL
                } else {
                    COBALT
                };
                let cx = pass.draw(s, dx + 18, r, dw - 21, WHITE, bg);
                if *focus == 1 || net.is_some() {
                    s.cursor = Some((cx, r));
                }
                r += 2;
                let hint = if !pass.value.is_empty() && pass.value.len() < 8 {
                    ("At least 8 characters.", BRICK)
                } else {
                    ("Enter to connect, Esc to cancel.", BLACK)
                };
                s.text(dx + 3, r, hint.0, hint.1, SILVER);
            }
            Modal::Message(title, lines) => {
                let dw = (lines.iter().map(|l| l.chars().count()).max().unwrap_or(20) as u16 + 8)
                    .max(30)
                    .min(s.cols - 4);
                let dh = lines.len() as u16 + 5;
                let dx = (s.cols - dw) / 2;
                let dy = (s.rows - dh) / 2;
                s.dialog(dx, dy, dw, dh, title, BLACK, SILVER);
                for (i, l) in lines.iter().enumerate() {
                    s.text(
                        dx + 3,
                        dy + 2 + i as u16,
                        &clip(l, dw as usize - 6),
                        BLACK,
                        SILVER,
                    );
                }
                let ok = "< OK >";
                s.text(
                    dx + (dw - ok.len() as u16) / 2,
                    dy + dh - 2,
                    ok,
                    BLACK,
                    TEAL,
                );
            }
            Modal::None => {}
        }
        if let Some(b) = &self.busy {
            let dw = (b.chars().count() as u16 + 8).max(30).min(s.cols - 4);
            let dx = (s.cols - dw) / 2;
            let dy = s.rows / 2 - 2;
            s.dialog(dx, dy, dw, 5, "Please Wait", BLACK, SILVER);
            s.text(dx + 3, dy + 2, b, BLACK, SILVER);
            s.cursor = None;
        }
    }
}

fn clip(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(w.saturating_sub(1)).collect();
        t.push('»');
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Fake {
        log: Rc<RefCell<Vec<String>>>,
    }

    impl WifiApi for Fake {
        fn device(&self) -> String {
            "wlan0".into()
        }
        fn state(&self) -> String {
            "disconnected".into()
        }
        fn scanning(&self) -> bool {
            false
        }
        fn scan(&self) -> Result<(), String> {
            self.log.borrow_mut().push("scan".into());
            Ok(())
        }
        fn networks(&self) -> Result<Vec<Network>, String> {
            let n = |ssid: &str, security, dbm, known| Network {
                ssid: ssid.into(),
                path: format!("/{ssid}"),
                security,
                dbm,
                connected: false,
                known,
            };
            Ok(vec![
                n("HomeNet", Security::Psk, -50, false),
                n("Cafe-Guest", Security::Open, -72, false),
                n("Office", Security::Psk, -80, true),
            ])
        }
        fn connect(&self, net: &Network, pass: Option<String>) -> Result<(), String> {
            self.log
                .borrow_mut()
                .push(format!("connect {} {:?}", net.ssid, pass));
            Ok(())
        }
        fn connect_hidden(&self, ssid: &str, pass: Option<String>) -> Result<(), String> {
            self.log
                .borrow_mut()
                .push(format!("hidden {ssid} {pass:?}"));
            Ok(())
        }
        fn disconnect(&self) -> Result<(), String> {
            Ok(())
        }
        fn forget(&self, net: &Network) -> Result<(), String> {
            self.log.borrow_mut().push(format!("forget {}", net.ssid));
            Ok(())
        }
    }

    fn app() -> (App, Rc<RefCell<Vec<String>>>) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut a = App::new(Some(Box::new(Fake { log: log.clone() })), None);
        a.refresh(retro_sys::net::parse_describe(
            r#"{"Interfaces":[{"Name":"end0","Type":"ether","OperationalState":"routable","Addresses":[{"Address":[10,0,2,15],"PrefixLength":24}]}]}"#,
        ));
        (a, log)
    }

    fn step(a: &mut App, k: Key) {
        if let Some(j) = a.key(k) {
            a.run(j);
        }
    }

    #[test]
    fn join_psk_open_known_hidden() {
        let (mut a, log) = app();
        step(&mut a, Key::Enter); // HomeNet asks for a passphrase
        for c in "short".chars() {
            step(&mut a, Key::Char(c));
        }
        step(&mut a, Key::Enter); // too short: stays open
        assert!(matches!(a.modal, Modal::Join { .. }));
        for c in "-enough".chars() {
            step(&mut a, Key::Char(c));
        }
        step(&mut a, Key::Enter);
        step(&mut a, Key::Down);
        step(&mut a, Key::Enter); // open network: straight in
        step(&mut a, Key::Down);
        step(&mut a, Key::Enter); // known: no passphrase
        step(&mut a, Key::Char('f')); // Forget the known one
        step(&mut a, Key::Char('h'));
        for c in "Secret".chars() {
            step(&mut a, Key::Char(c));
        }
        step(&mut a, Key::Tab);
        for c in "password1".chars() {
            step(&mut a, Key::Char(c));
        }
        step(&mut a, Key::Enter);
        assert_eq!(
            *log.borrow(),
            [
                "connect HomeNet Some(\"short-enough\")",
                "connect Cafe-Guest None",
                "connect Office None",
                "forget Office",
                "hidden Secret Some(\"password1\")",
            ]
        );
    }

    #[test]
    fn draws_wired_and_wireless() {
        let (a, _) = app();
        let mut s = Screen::with_size(120, 33, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("end0") && t.contains("Connected") && t.contains("10.0.2.15/24"),
            "{t}"
        );
        assert!(t.contains("► HomeNet") && t.contains("▂▄▆█") && t.contains("Saved"));
        assert!(t.contains("< Join >") && t.contains("< Close >"));
    }
}
