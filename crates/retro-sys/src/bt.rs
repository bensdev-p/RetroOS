//! Bluetooth through BlueZ over the system D-Bus (DESIGN.md §10.2, D-054).
//!
//! `bluetoothd` runs only while Bluetooth is turned on: [`set_enabled`]
//! enables or disables `bluetooth.service` through the `set-bluetooth`
//! helper, so a RetroOS that never uses Bluetooth carries no daemon.
//!
//! Pairing, connecting and removing are slow (seconds, and pairing a
//! keyboard waits for the user to type a number on it), so they run on a
//! thread: [`BtApi::start`] begins a [`Job`], the front-end keeps drawing
//! and polls [`BtApi::prompt`] (a number to show or confirm, from the
//! pairing agent) and [`BtApi::take_result`]. After pairing, the device is
//! trusted (it reconnects by itself at the next start) and connected.

use crate::admin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::blocking::{fdo::ObjectManagerProxy, Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const BLUEZ: &str = "org.bluez";
const AGENT_PATH: &str = "/org/retroos/BtAgent";
/// How long a "does this number match?" question waits for an answer.
const CONFIRM_WAIT: Duration = Duration::from_secs(60);

/// What sort of thing a device is (from BlueZ's icon name).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyboard,
    Mouse,
    Gamepad,
    Audio,
    Phone,
    Computer,
    Other,
}

impl Kind {
    pub fn from_icon(icon: &str) -> Kind {
        match icon {
            "input-keyboard" => Kind::Keyboard,
            "input-mouse" | "input-tablet" => Kind::Mouse,
            "input-gaming" => Kind::Gamepad,
            i if i.starts_with("audio-") => Kind::Audio,
            "phone" => Kind::Phone,
            "computer" => Kind::Computer,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Keyboard => "Keyboard",
            Kind::Mouse => "Mouse",
            Kind::Gamepad => "Game pad",
            Kind::Audio => "Audio",
            Kind::Phone => "Phone",
            Kind::Computer => "Computer",
            Kind::Other => "Device",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The D-Bus object ("/org/bluez/hci0/dev_AA_BB_…").
    pub path: String,
    pub address: String,
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    /// Signal strength in dBm while searching.
    pub rssi: Option<i16>,
}

impl Device {
    pub fn state(&self) -> &'static str {
        match (self.connected, self.paired) {
            (true, _) => "Connected",
            (false, true) => "Paired",
            _ => "",
        }
    }
}

/// A slow call, run on a thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Pair (if not yet), trust, connect.
    Pair(Device),
    Connect(Device),
    Disconnect(Device),
    /// Forget the pairing.
    Remove(Device),
}

impl Job {
    pub fn busy_text(&self) -> String {
        match self {
            Job::Pair(d) => format!("Pairing with {}...", d.name),
            Job::Connect(d) => format!("Connecting to {}...", d.name),
            Job::Disconnect(d) => format!("Disconnecting {}...", d.name),
            Job::Remove(d) => format!("Removing {}...", d.name),
        }
    }

    /// What to say when it worked.
    pub fn done_text(&self) -> String {
        match self {
            Job::Pair(d) => format!("Paired with {}.", d.name),
            Job::Connect(d) => format!("{} is connected.", d.name),
            Job::Disconnect(d) => format!("{} is disconnected.", d.name),
            Job::Remove(d) => format!("{} was removed.", d.name),
        }
    }

    pub fn device(&self) -> &Device {
        match self {
            Job::Pair(d) | Job::Connect(d) | Job::Disconnect(d) | Job::Remove(d) => d,
        }
    }
}

/// What the pairing agent needs the user to see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// Type `code` on the device (a keyboard), then Enter.
    Type { device: String, code: String },
    /// Does `code` show on the device too? Answer with [`BtApi::answer`].
    Confirm { device: String, code: String },
}

impl Prompt {
    /// The prompt's text, given the device's name.
    pub fn lines(&self, name: &str) -> Vec<String> {
        match self {
            Prompt::Type { code, .. } => vec![
                format!("On {name}, type"),
                format!("{code}"),
                "and press Enter.".into(),
            ],
            Prompt::Confirm { code, .. } => vec![
                format!("Does {name} show"),
                format!("{code}"),
                "too?".into(),
            ],
        }
    }

    pub fn device(&self) -> &str {
        match self {
            Prompt::Type { device, .. } | Prompt::Confirm { device, .. } => device,
        }
    }
}

/// What the Bluetooth screens need from the system (a fake one in tests).
pub trait BtApi {
    /// The adapter's name ("hci0") and address.
    fn adapter(&self) -> String;
    fn powered(&self) -> bool;
    fn set_powered(&self, on: bool) -> Result<(), String>;
    fn discovering(&self) -> bool;
    /// Start or stop searching for devices.
    fn discover(&self, on: bool) -> Result<(), String>;
    /// Paired devices first, then those found, strongest first.
    fn devices(&self) -> Result<Vec<Device>, String>;
    /// Begin a slow job; one at a time.
    fn start(&self, job: Job);
    /// The job running, if any.
    fn busy(&self) -> Option<Job>;
    /// The finished job and its outcome (taken once).
    fn take_result(&self) -> Option<(Job, Result<(), String>)>;
    fn prompt(&self) -> Option<Prompt>;
    /// The user's answer to a [`Prompt::Confirm`].
    fn answer(&self, yes: bool);
    /// Give up pairing (Esc while a number shows).
    fn cancel(&self);
}

/// Is Bluetooth turned on (bluetoothd enabled to run)?
pub fn enabled() -> bool {
    if std::env::var_os("RETROOS_TEST_BT").is_some() {
        return true;
    }
    std::process::Command::new("systemctl")
        .args(["is-enabled", "--quiet", "bluetooth.service"])
        .status()
        .is_ok_and(|s| s.success())
}

/// Turn Bluetooth on (start bluetoothd now and at every start) or off.
pub fn set_enabled(on: bool) -> Result<(), String> {
    if std::env::var_os("RETROOS_TEST_BT").is_some() {
        return Ok(());
    }
    admin::run_helper("set-bluetooth", &[if on { "on" } else { "off" }]).map_err(|e| e.to_string())
}

fn err(e: zbus::Error) -> String {
    let s = e.to_string();
    if s.contains("AuthenticationFailed") || s.contains("AuthenticationRejected") {
        "Pairing failed. Try again; type the number exactly, then Enter.".into()
    } else if s.contains("AuthenticationCanceled") || s.contains("Canceled") {
        "Pairing was cancelled.".into()
    } else if s.contains("AuthenticationTimeout") || s.contains("Timeout") || s.contains("NoReply")
    {
        "The device did not answer in time. Try again.".into()
    } else if s.contains("ConnectionAttemptFailed")
        || s.contains("page-timeout")
        || s.contains("Host is down")
    {
        "The device did not answer. Make sure it is on and ready to pair.".into()
    } else if s.contains("profile-unavailable") || s.contains("NotAvailable") {
        "RetroOS has nothing that can use this device.".into()
    } else if s.contains("InProgress") || s.contains("Busy") {
        "Bluetooth is busy. Try again in a moment.".into()
    } else if s.contains("NotReady") || s.contains("rfkill") {
        "The Bluetooth adapter is not ready (switched off?).".into()
    } else if s.contains("system_bus_socket") {
        "System services are not running (no D-Bus system bus).".into()
    } else if s.contains("AccessDenied") {
        "Bluetooth is not available to this user.".into()
    } else {
        s
    }
}

type Objects =
    HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>>;

fn s_of(v: &OwnedValue) -> Option<String> {
    <&str>::try_from(&**v).ok().map(String::from)
}

fn b_of(v: &OwnedValue) -> Option<bool> {
    bool::try_from(&**v).ok()
}

fn i16_of(v: &OwnedValue) -> Option<i16> {
    i16::try_from(&**v).ok()
}

/// A device from BlueZ's Device1 properties.
pub fn device_from(path: &str, props: &HashMap<String, OwnedValue>) -> Device {
    let address = props.get("Address").and_then(s_of).unwrap_or_default();
    let name = props
        .get("Alias")
        .and_then(s_of)
        .or_else(|| props.get("Name").and_then(s_of))
        .unwrap_or_else(|| address.clone());
    Device {
        path: path.to_string(),
        name,
        kind: Kind::from_icon(&props.get("Icon").and_then(s_of).unwrap_or_default()),
        paired: props.get("Paired").and_then(b_of).unwrap_or(false),
        connected: props.get("Connected").and_then(b_of).unwrap_or(false),
        rssi: props.get("RSSI").and_then(i16_of),
        address,
    }
}

/// Paired first, then connected, then by signal; nameless strangers (BlueZ
/// names them by their address) only at the end.
pub fn sort_devices(v: &mut [Device]) {
    v.sort_by_key(|d| {
        (
            !d.paired,
            !d.connected,
            d.name == d.address.replace(':', "-") || d.name == d.address,
            -(d.rssi.unwrap_or(-127) as i32),
            d.name.to_lowercase(),
        )
    });
}

/// A random six-digit number for a keyboard to type.
fn random_code() -> u32 {
    let mut b = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    u32::from_le_bytes(b) % 1_000_000
}

/// What the agent, the job thread and the front-end share.
#[derive(Default)]
struct Shared {
    prompt: Option<Prompt>,
    answer: Option<bool>,
    busy: Option<Job>,
    result: Option<(Job, Result<(), String>)>,
}

type SharedRef = Arc<Mutex<Shared>>;

fn with<T>(s: &SharedRef, f: impl FnOnce(&mut Shared) -> T) -> T {
    let mut g = s.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut g)
}

/// The pairing agent (org.bluez.Agent1, capability KeyboardDisplay).
struct Agent {
    shared: SharedRef,
}

impl Agent {
    /// The device being paired is a keyboard (it gets a number to type).
    fn keyboard(&self, device: &OwnedObjectPath) -> bool {
        with(
            &self.shared,
            |s| matches!(&s.busy, Some(Job::Pair(d)) if d.path == device.as_str() && d.kind == Kind::Keyboard),
        )
    }

    fn show(&self, device: &OwnedObjectPath, code: String) {
        with(&self.shared, |s| {
            s.prompt = Some(Prompt::Type {
                device: device.to_string(),
                code,
            })
        });
    }
}

fn rejected() -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied("Rejected".into())
}

#[zbus::interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {}

    /// Old (before 2.1) devices: a keyboard types a fresh PIN; headsets
    /// and the like almost all answer to 0000.
    fn request_pin_code(&self, device: OwnedObjectPath) -> String {
        if self.keyboard(&device) {
            let code = format!("{:06}", random_code());
            self.show(&device, code.clone());
            code
        } else {
            "0000".into()
        }
    }

    fn display_pin_code(&self, device: OwnedObjectPath, pincode: String) {
        self.show(&device, pincode);
    }

    fn request_passkey(&self, device: OwnedObjectPath) -> u32 {
        let code = random_code();
        self.show(&device, format!("{code:06}"));
        code
    }

    fn display_passkey(&self, device: OwnedObjectPath, passkey: u32, _entered: u16) {
        self.show(&device, format!("{passkey:06}"));
    }

    /// Both sides show a number; the user says whether they match.
    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
    ) -> zbus::fdo::Result<()> {
        with(&self.shared, |s| {
            s.answer = None;
            s.prompt = Some(Prompt::Confirm {
                device: device.to_string(),
                code: format!("{passkey:06}"),
            });
        });
        let until = Instant::now() + CONFIRM_WAIT;
        loop {
            if let Some(yes) = with(&self.shared, |s| s.answer.take()) {
                with(&self.shared, |s| s.prompt = None);
                return if yes { Ok(()) } else { Err(rejected()) };
            }
            if Instant::now() > until {
                with(&self.shared, |s| s.prompt = None);
                return Err(rejected());
            }
            async_io::Timer::after(Duration::from_millis(100)).await;
        }
    }

    /// "Just works" pairing the user started here: allow it.
    fn request_authorization(&self, _device: OwnedObjectPath) {}

    /// A paired device using a service: allow it.
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}

    fn cancel(&self) {
        with(&self.shared, |s| s.prompt = None);
    }
}

#[derive(Clone)]
pub struct Bt {
    conn: Connection,
    adapter: String,
    shared: SharedRef,
}

/// Why there is no [`Bt`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Absent {
    /// Bluetooth is turned off (bluetoothd not running).
    Off,
    /// bluetoothd runs but found no adapter.
    NoAdapter,
    Error(String),
}

impl Absent {
    pub fn text(&self) -> String {
        match self {
            Absent::Off => "Bluetooth is off.".into(),
            Absent::NoAdapter => "There is no Bluetooth adapter.".into(),
            Absent::Error(e) => e.clone(),
        }
    }
}

impl Bt {
    /// The first adapter, with our pairing agent registered.
    pub fn open() -> Result<Bt, Absent> {
        let conn = Connection::system().map_err(|e| Absent::Error(err(e)))?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&conn).map_err(|e| Absent::Error(err(e)))?;
        let name =
            zbus::names::BusName::try_from(BLUEZ).map_err(|e| Absent::Error(e.to_string()))?;
        if !dbus.name_has_owner(name).unwrap_or(false) {
            return Err(Absent::Off);
        }
        let objs = objects(&conn).map_err(Absent::Error)?;
        let Some(adapter) = find(&objs, "org.bluez.Adapter1").into_iter().next() else {
            return Err(Absent::NoAdapter);
        };
        let shared = SharedRef::default();
        conn.object_server()
            .at(
                AGENT_PATH,
                Agent {
                    shared: shared.clone(),
                },
            )
            .map_err(|e| Absent::Error(err(e)))?;
        if let Ok(am) = Proxy::new(&conn, BLUEZ, "/org/bluez", "org.bluez.AgentManager1") {
            if let Ok(p) = zbus::zvariant::ObjectPath::try_from(AGENT_PATH) {
                let _: Result<(), _> = am.call("RegisterAgent", &(p.clone(), "KeyboardDisplay"));
                let _: Result<(), _> = am.call("RequestDefaultAgent", &(p,));
            }
        }
        Ok(Bt {
            conn,
            adapter: adapter.0,
            shared,
        })
    }

    /// Wait (up to `max`) for bluetoothd and its adapter after turning
    /// Bluetooth on.
    pub fn open_wait(max: Duration) -> Result<Bt, Absent> {
        let until = Instant::now() + max;
        loop {
            match Bt::open() {
                Err(Absent::Off | Absent::NoAdapter) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(200))
                }
                r => return r,
            }
        }
    }

    fn proxy(&self, path: &str, iface: &'static str) -> Result<Proxy<'_>, String> {
        Proxy::new(&self.conn, BLUEZ, path.to_string(), iface).map_err(err)
    }

    fn adapter_proxy(&self) -> Result<Proxy<'_>, String> {
        self.proxy(&self.adapter.clone(), "org.bluez.Adapter1")
    }

    fn run(&self, job: &Job) -> Result<(), String> {
        let d = job.device();
        let dev = self.proxy(&d.path, "org.bluez.Device1")?;
        match job {
            Job::Pair(_) => {
                // Searching makes pairing unreliable on many adapters.
                let _ = self.discover(false);
                if !dev.get_property::<bool>("Paired").unwrap_or(false) {
                    match dev.call::<_, _, ()>("Pair", &()) {
                        Ok(()) => {}
                        Err(e) if e.to_string().contains("AlreadyExists") => {}
                        Err(e) => return Err(err(e)),
                    }
                }
                with(&self.shared, |s| s.prompt = None);
                dev.set_property("Trusted", true)
                    .map_err(|e| err(e.into()))?;
                // Some devices connect themselves after pairing.
                match dev.call::<_, _, ()>("Connect", &()) {
                    Ok(()) => Ok(()),
                    Err(e) if e.to_string().contains("AlreadyConnected") => Ok(()),
                    Err(e) => Err(err(e)),
                }
            }
            Job::Connect(_) => match dev.call::<_, _, ()>("Connect", &()) {
                Err(e) if !e.to_string().contains("AlreadyConnected") => Err(err(e)),
                _ => Ok(()),
            },
            Job::Disconnect(_) => dev.call::<_, _, ()>("Disconnect", &()).map_err(err),
            Job::Remove(_) => {
                let p = OwnedObjectPath::try_from(d.path.as_str()).map_err(|e| e.to_string())?;
                self.adapter_proxy()?
                    .call::<_, _, ()>("RemoveDevice", &(p,))
                    .map_err(err)
            }
        }
    }
}

fn objects(conn: &Connection) -> Result<Objects, String> {
    let om = ObjectManagerProxy::builder(conn)
        .destination(BLUEZ)
        .map_err(err)?
        .path("/")
        .map_err(err)?
        .build()
        .map_err(err)?;
    om.get_managed_objects().map_err(|e| err(e.into()))
}

fn find<'a>(objs: &'a Objects, iface: &str) -> Vec<(String, &'a HashMap<String, OwnedValue>)> {
    let mut v: Vec<_> = objs
        .iter()
        .filter_map(|(p, ifs)| {
            ifs.iter()
                .find(|(n, _)| n.as_str() == iface)
                .map(|(_, props)| (p.to_string(), props))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

impl BtApi for Bt {
    fn adapter(&self) -> String {
        let addr = self
            .adapter_proxy()
            .and_then(|p| p.get_property::<String>("Address").map_err(err))
            .unwrap_or_default();
        let name = self.adapter.rsplit('/').next().unwrap_or("hci0");
        if addr.is_empty() {
            name.into()
        } else {
            format!("{name}, {addr}")
        }
    }

    fn powered(&self) -> bool {
        self.adapter_proxy()
            .and_then(|p| p.get_property::<bool>("Powered").map_err(err))
            .unwrap_or(false)
    }

    fn set_powered(&self, on: bool) -> Result<(), String> {
        self.adapter_proxy()?
            .set_property("Powered", on)
            .map_err(|e| err(e.into()))
    }

    fn discovering(&self) -> bool {
        self.adapter_proxy()
            .and_then(|p| p.get_property::<bool>("Discovering").map_err(err))
            .unwrap_or(false)
    }

    fn discover(&self, on: bool) -> Result<(), String> {
        let p = self.adapter_proxy()?;
        let r = p.call::<_, _, ()>(
            if on {
                "StartDiscovery"
            } else {
                "StopDiscovery"
            },
            &(),
        );
        match r {
            // Already searching / not searching is fine.
            Err(e)
                if e.to_string().contains("InProgress")
                    || e.to_string().contains("NotReady") && !on =>
            {
                Ok(())
            }
            Err(e) if !on && e.to_string().contains("Failed") => Ok(()),
            r => r.map_err(err),
        }
    }

    fn devices(&self) -> Result<Vec<Device>, String> {
        let objs = objects(&self.conn)?;
        let prefix = format!("{}/", self.adapter);
        let mut v: Vec<Device> = find(&objs, "org.bluez.Device1")
            .into_iter()
            .filter(|(p, _)| p.starts_with(&prefix))
            .map(|(p, props)| device_from(&p, props))
            .collect();
        sort_devices(&mut v);
        Ok(v)
    }

    fn start(&self, job: Job) {
        let busy = with(&self.shared, |s| {
            if s.busy.is_some() {
                return true;
            }
            s.busy = Some(job.clone());
            s.result = None;
            s.prompt = None;
            false
        });
        if busy {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            let r = me.run(&job);
            with(&me.shared, |s| {
                s.busy = None;
                s.prompt = None;
                s.result = Some((job, r));
            });
        });
    }

    fn busy(&self) -> Option<Job> {
        with(&self.shared, |s| s.busy.clone())
    }

    fn take_result(&self) -> Option<(Job, Result<(), String>)> {
        with(&self.shared, |s| s.result.take())
    }

    fn prompt(&self) -> Option<Prompt> {
        with(&self.shared, |s| s.prompt.clone())
    }

    fn answer(&self, yes: bool) {
        with(&self.shared, |s| s.answer = Some(yes));
    }

    fn cancel(&self) {
        if let Some(Job::Pair(d)) = self.busy() {
            // The job thread's Pair call then fails with "Canceled".
            if let Ok(p) = self.proxy(&d.path, "org.bluez.Device1") {
                let _: Result<(), _> = p.call("CancelPairing", &());
            }
        }
        with(&self.shared, |s| {
            s.answer = Some(false);
            s.prompt = None;
        });
    }
}

/// The adapter for the front-ends: a pretend one under `RETROOS_TEST_BT=1`
/// (host tests), else BlueZ's.
pub fn open_api() -> Result<Box<dyn BtApi>, Absent> {
    if std::env::var_os("RETROOS_TEST_BT").is_some() {
        return Ok(Box::new(fake::Fake::default()));
    }
    Bt::open().map(|b| Box::new(b) as Box<dyn BtApi>)
}

/// A pretend adapter for host tests: a paired pair of headphones, a
/// keyboard and a game pad nearby. Pairing the keyboard shows a number to
/// type for three seconds, then succeeds.
pub mod fake {
    use super::*;
    use std::cell::RefCell;

    pub struct Fake {
        devs: RefCell<Vec<Device>>,
        job: RefCell<Option<(Job, Instant)>>,
        result: RefCell<Option<(Job, Result<(), String>)>>,
        discovering: RefCell<bool>,
    }

    fn dev(name: &str, kind: Kind, paired: bool, connected: bool, rssi: i16) -> Device {
        let n = name.len() as u8;
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            address: format!("00:1A:7D:DA:71:{n:02X}"),
            name: name.into(),
            kind,
            paired,
            connected,
            rssi: Some(rssi),
        }
    }

    impl Default for Fake {
        fn default() -> Fake {
            Fake {
                devs: RefCell::new(vec![
                    dev("Studio Headphones", Kind::Audio, true, true, -50),
                    dev("Compact Keyboard", Kind::Keyboard, false, false, -58),
                    dev("Game Pad", Kind::Gamepad, false, false, -66),
                ]),
                job: RefCell::new(None),
                result: RefCell::new(None),
                discovering: RefCell::new(false),
            }
        }
    }

    impl Fake {
        /// Finish the job once its time is up.
        fn tick(&self) {
            let done = matches!(&*self.job.borrow(), Some((j, t))
                if t.elapsed() >= Duration::from_millis(if matches!(j, Job::Pair(d) if d.kind == Kind::Keyboard) { 3000 } else { 500 }));
            if !done {
                return;
            }
            let Some((job, _)) = self.job.borrow_mut().take() else {
                return;
            };
            let mut devs = self.devs.borrow_mut();
            let name = job.device().name.clone();
            match &job {
                Job::Pair(_) | Job::Connect(_) => {
                    for d in devs.iter_mut().filter(|d| d.name == name) {
                        d.paired = true;
                        d.connected = true;
                    }
                }
                Job::Disconnect(_) => {
                    for d in devs.iter_mut().filter(|d| d.name == name) {
                        d.connected = false;
                    }
                }
                Job::Remove(_) => {
                    for d in devs.iter_mut().filter(|d| d.name == name) {
                        d.paired = false;
                        d.connected = false;
                    }
                }
            }
            sort_devices(&mut devs);
            *self.result.borrow_mut() = Some((job, Ok(())));
        }
    }

    impl BtApi for Fake {
        fn adapter(&self) -> String {
            "hci0, 2C:CF:67:00:00:01".into()
        }
        fn powered(&self) -> bool {
            true
        }
        fn set_powered(&self, _on: bool) -> Result<(), String> {
            Ok(())
        }
        fn discovering(&self) -> bool {
            *self.discovering.borrow()
        }
        fn discover(&self, on: bool) -> Result<(), String> {
            *self.discovering.borrow_mut() = on;
            Ok(())
        }
        fn devices(&self) -> Result<Vec<Device>, String> {
            self.tick();
            Ok(self.devs.borrow().clone())
        }
        fn start(&self, job: Job) {
            if self.job.borrow().is_none() {
                *self.job.borrow_mut() = Some((job, Instant::now()));
            }
        }
        fn busy(&self) -> Option<Job> {
            self.tick();
            self.job.borrow().as_ref().map(|(j, _)| j.clone())
        }
        fn take_result(&self) -> Option<(Job, Result<(), String>)> {
            self.tick();
            self.result.borrow_mut().take()
        }
        fn prompt(&self) -> Option<Prompt> {
            match &*self.job.borrow() {
                Some((Job::Pair(d), _)) if d.kind == Kind::Keyboard => Some(Prompt::Type {
                    device: d.path.clone(),
                    code: "314159".into(),
                }),
                _ => None,
            }
        }
        fn answer(&self, _yes: bool) {}
        fn cancel(&self) {
            if let Some((job, _)) = self.job.borrow_mut().take() {
                *self.result.borrow_mut() = Some((job, Err("Pairing was cancelled.".into())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn ov(v: Value<'_>) -> OwnedValue {
        v.try_to_owned().unwrap()
    }

    #[test]
    fn devices_from_bluez_properties() {
        let mut p = HashMap::new();
        p.insert("Address".to_string(), ov(Value::from("AA:BB:CC:DD:EE:FF")));
        p.insert("Alias".to_string(), ov(Value::from("My Keyboard")));
        p.insert("Icon".to_string(), ov(Value::from("input-keyboard")));
        p.insert("Paired".to_string(), ov(Value::from(true)));
        p.insert("RSSI".to_string(), ov(Value::from(-60i16)));
        let d = device_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF", &p);
        assert_eq!(d.name, "My Keyboard");
        assert_eq!(d.kind, Kind::Keyboard);
        assert!(d.paired && !d.connected);
        assert_eq!(d.rssi, Some(-60));
        assert_eq!(d.state(), "Paired");
        let mut q = HashMap::new();
        q.insert("Address".to_string(), ov(Value::from("11:22:33:44:55:66")));
        let n = device_from("/x", &q);
        assert_eq!(n.name, "11:22:33:44:55:66", "no name: the address");
        assert_eq!(n.kind, Kind::Other);
    }

    #[test]
    fn kinds_from_icons() {
        assert_eq!(Kind::from_icon("audio-headphones"), Kind::Audio);
        assert_eq!(Kind::from_icon("audio-card"), Kind::Audio);
        assert_eq!(Kind::from_icon("input-gaming"), Kind::Gamepad);
        assert_eq!(Kind::from_icon("input-tablet"), Kind::Mouse);
        assert_eq!(Kind::from_icon(""), Kind::Other);
    }

    #[test]
    fn paired_first_then_strongest_then_nameless() {
        let d = |name: &str, paired, rssi| Device {
            path: name.into(),
            address: "AA:BB:CC:DD:EE:FF".into(),
            name: name.into(),
            kind: Kind::Other,
            paired,
            connected: false,
            rssi: Some(rssi),
        };
        let mut v = vec![
            d("AA-BB-CC-DD-EE-FF", false, -30),
            d("Far", false, -80),
            d("Near", false, -40),
            d("Mine", true, -90),
        ];
        sort_devices(&mut v);
        let names: Vec<&str> = v.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Mine", "Near", "Far", "AA-BB-CC-DD-EE-FF"]);
    }

    #[test]
    fn the_fake_pairs_a_keyboard_with_a_number() {
        let f = fake::Fake::default();
        let devs = f.devices().unwrap();
        let kb = devs
            .iter()
            .find(|d| d.kind == Kind::Keyboard)
            .unwrap()
            .clone();
        f.start(Job::Pair(kb));
        assert!(f.busy().is_some());
        let p = f.prompt().unwrap();
        assert_eq!(p.lines("Compact Keyboard")[1], "314159");
        assert!(f.take_result().is_none());
        f.cancel();
        assert!(f.busy().is_none() && f.prompt().is_none());
        assert_eq!(
            f.take_result().unwrap().1,
            Err("Pairing was cancelled.".into())
        );
    }
}
