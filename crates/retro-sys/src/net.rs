//! Networking: Wi-Fi through iwd, wired links through systemd-networkd,
//! both over the system D-Bus (DESIGN.md §10.1).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zbus::blocking::{fdo::ObjectManagerProxy, Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const IWD: &str = "net.connman.iwd";
const AGENT_PATH: &str = "/org/retroos/NetAgent";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    Psk,
    Enterprise,
    Other,
}

impl Security {
    pub fn label(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::Psk => "WPA2/3",
            Security::Enterprise => "802.1X",
            Security::Other => "?",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Network {
    pub ssid: String,
    pub path: String,
    pub security: Security,
    /// Signal in dBm (e.g. -55).
    pub dbm: i16,
    pub connected: bool,
    pub known: bool,
}

impl Network {
    /// 0–4 bars, like a phone.
    pub fn bars(&self) -> u8 {
        match self.dbm {
            d if d >= -55 => 4,
            d if d >= -67 => 3,
            d if d >= -75 => 2,
            d if d >= -85 => 1,
            _ => 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub kind: String,
    /// networkd's operational state: "routable", "no-carrier", "off", …
    pub state: String,
    pub addresses: Vec<String>,
}

impl Link {
    pub fn friendly_state(&self) -> &'static str {
        match self.state.as_str() {
            "routable" | "enslaved" => "Connected",
            "degraded" => "Limited (no address)",
            "carrier" | "dormant" => "Cable connected, configuring",
            "no-carrier" => "Cable unplugged",
            "off" => "Off",
            _ => "Unknown",
        }
    }
}

/// What the network screens need from the system (a fake one in tests).
pub trait WifiApi {
    fn device(&self) -> String;
    fn state(&self) -> String;
    fn scanning(&self) -> bool;
    fn scan(&self) -> Result<(), String>;
    fn networks(&self) -> Result<Vec<Network>, String>;
    /// Connect; `passphrase` is handed to iwd when it asks for one.
    fn connect(&self, net: &Network, passphrase: Option<String>) -> Result<(), String>;
    fn connect_hidden(&self, ssid: &str, passphrase: Option<String>) -> Result<(), String>;
    fn disconnect(&self) -> Result<(), String>;
    fn forget(&self, net: &Network) -> Result<(), String>;
}

fn err(e: zbus::Error) -> String {
    let s = e.to_string();
    // "net.connman.iwd.Failed: Operation failed" → friendlier words.
    if s.contains("net.connman.iwd.Failed") || s.contains("Canceled") {
        "The connection failed. Check the passphrase and try again.".into()
    } else if s.contains("InvalidFormat") {
        "That passphrase is not valid (8 to 63 characters).".into()
    } else if s.contains("NotFound") {
        "The network is out of range.".into()
    } else if s.contains("Busy") || s.contains("InProgress") {
        "The adapter is busy. Try again in a moment.".into()
    } else if s.contains("system_bus_socket") {
        "System services are not running (no D-Bus system bus).".into()
    } else if s.contains("AccessDenied") || s.contains("ServiceUnknown") {
        "The wireless service is not available to this user.".into()
    } else {
        s
    }
}

fn s_of(v: &OwnedValue) -> Option<String> {
    <&str>::try_from(&**v).ok().map(String::from)
}

fn b_of(v: &OwnedValue) -> Option<bool> {
    bool::try_from(&**v).ok()
}

fn path_of(v: &OwnedValue) -> Option<String> {
    match &**v {
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

type Objects =
    HashMap<OwnedObjectPath, HashMap<zbus::names::OwnedInterfaceName, HashMap<String, OwnedValue>>>;

/// iwd's interface `iface` on any object, with its properties.
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

struct Agent {
    secret: Arc<Mutex<Option<String>>>,
}

#[zbus::interface(name = "net.connman.iwd.Agent")]
impl Agent {
    fn release(&self) {}

    fn request_passphrase(&self, _network: OwnedObjectPath) -> zbus::fdo::Result<String> {
        self.secret
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .ok_or_else(|| zbus::fdo::Error::Failed("Canceled".into()))
    }

    fn cancel(&self, _reason: String) {}
}

pub struct Wifi {
    conn: Connection,
    station: String,
    device: String,
    secret: Arc<Mutex<Option<String>>>,
}

impl Wifi {
    /// The first Wi-Fi device iwd manages. `Ok(None)`: no adapter.
    pub fn open() -> Result<Option<Wifi>, String> {
        let conn = Connection::system().map_err(err)?;
        let objs = Self::objects(&conn)?;
        let Some((station, _)) = find(&objs, "net.connman.iwd.Station").into_iter().next() else {
            return Ok(None);
        };
        let device = find(&objs, "net.connman.iwd.Device")
            .into_iter()
            .find(|(p, _)| *p == station)
            .and_then(|(_, props)| props.get("Name").and_then(s_of))
            .unwrap_or_else(|| "wlan0".into());
        let secret = Arc::new(Mutex::new(None));
        conn.object_server()
            .at(
                AGENT_PATH,
                Agent {
                    secret: secret.clone(),
                },
            )
            .map_err(err)?;
        let am = Proxy::new(
            &conn,
            IWD,
            "/net/connman/iwd",
            "net.connman.iwd.AgentManager",
        )
        .map_err(err)?;
        let path = zbus::zvariant::ObjectPath::try_from(AGENT_PATH).map_err(|e| e.to_string())?;
        // Another agent (an earlier Filer run, iwctl) may hold the slot; we still try.
        let _: Result<(), _> = am.call("RegisterAgent", &(path,));
        Ok(Some(Wifi {
            conn,
            station,
            device,
            secret,
        }))
    }

    fn objects(conn: &Connection) -> Result<Objects, String> {
        let om = ObjectManagerProxy::builder(conn)
            .destination(IWD)
            .map_err(err)?
            .path("/")
            .map_err(err)?
            .build()
            .map_err(err)?;
        om.get_managed_objects().map_err(|e| err(e.into()))
    }

    fn station(&self) -> Result<Proxy<'_>, String> {
        Proxy::new(
            &self.conn,
            IWD,
            self.station.as_str(),
            "net.connman.iwd.Station",
        )
        .map_err(err)
    }

    fn with_secret<T>(
        &self,
        pass: Option<String>,
        f: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        if let Ok(mut s) = self.secret.lock() {
            *s = pass;
        }
        let r = f();
        if let Ok(mut s) = self.secret.lock() {
            *s = None;
        }
        r
    }
}

impl WifiApi for Wifi {
    fn device(&self) -> String {
        self.device.clone()
    }

    fn state(&self) -> String {
        self.station()
            .and_then(|p| p.get_property::<String>("State").map_err(err))
            .unwrap_or_else(|_| "unknown".into())
    }

    fn scanning(&self) -> bool {
        self.station()
            .and_then(|p| p.get_property::<bool>("Scanning").map_err(err))
            .unwrap_or(false)
    }

    fn scan(&self) -> Result<(), String> {
        match self.station()?.call::<_, _, ()>("Scan", &()) {
            Ok(()) => Ok(()),
            // Already scanning is fine.
            Err(e) if e.to_string().contains("Busy") || e.to_string().contains("InProgress") => {
                Ok(())
            }
            Err(e) => Err(err(e)),
        }
    }

    fn networks(&self) -> Result<Vec<Network>, String> {
        let ordered: Vec<(OwnedObjectPath, i16)> = self
            .station()?
            .call("GetOrderedNetworks", &())
            .map_err(err)?;
        let objs = Self::objects(&self.conn)?;
        let nets = find(&objs, "net.connman.iwd.Network");
        Ok(ordered
            .into_iter()
            .filter_map(|(p, strength)| {
                let (path, props) = nets.iter().find(|(np, _)| *np == p.as_str())?;
                let security = match props.get("Type").and_then(s_of).as_deref() {
                    Some("open") => Security::Open,
                    Some("psk") => Security::Psk,
                    Some("8021x") => Security::Enterprise,
                    _ => Security::Other,
                };
                Some(Network {
                    ssid: props.get("Name").and_then(s_of).unwrap_or_default(),
                    path: path.clone(),
                    security,
                    dbm: strength / 100,
                    connected: props.get("Connected").and_then(b_of).unwrap_or(false),
                    known: props.get("KnownNetwork").and_then(path_of).is_some(),
                })
            })
            .collect())
    }

    fn connect(&self, net: &Network, passphrase: Option<String>) -> Result<(), String> {
        self.with_secret(passphrase, || {
            let p = Proxy::new(
                &self.conn,
                IWD,
                net.path.as_str(),
                "net.connman.iwd.Network",
            )
            .map_err(err)?;
            p.call::<_, _, ()>("Connect", &()).map_err(err)
        })
    }

    fn connect_hidden(&self, ssid: &str, passphrase: Option<String>) -> Result<(), String> {
        self.with_secret(passphrase, || {
            self.station()?
                .call::<_, _, ()>("ConnectHiddenNetwork", &(ssid,))
                .map_err(err)
        })
    }

    fn disconnect(&self) -> Result<(), String> {
        self.station()?
            .call::<_, _, ()>("Disconnect", &())
            .map_err(err)
    }

    fn forget(&self, net: &Network) -> Result<(), String> {
        let objs = Self::objects(&self.conn)?;
        let known = find(&objs, "net.connman.iwd.Network")
            .into_iter()
            .find(|(p, _)| *p == net.path)
            .and_then(|(_, props)| props.get("KnownNetwork").and_then(path_of))
            .ok_or("This network is not remembered.")?;
        let p = Proxy::new(
            &self.conn,
            IWD,
            known.as_str(),
            "net.connman.iwd.KnownNetwork",
        )
        .map_err(err)?;
        p.call::<_, _, ()>("Forget", &()).map_err(err)
    }
}

/// Wired (and other non-loopback) links as systemd-networkd sees them.
pub fn links() -> Result<Vec<Link>, String> {
    let conn = Connection::system().map_err(err)?;
    let p = Proxy::new(
        &conn,
        "org.freedesktop.network1",
        "/org/freedesktop/network1",
        "org.freedesktop.network1.Manager",
    )
    .map_err(err)?;
    let json: String = p.call("Describe", &()).map_err(err)?;
    Ok(parse_describe(&json))
}

/// Parse networkd's `Describe()` JSON.
pub fn parse_describe(json: &str) -> Vec<Link> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in v["Interfaces"].as_array().into_iter().flatten() {
        let kind = i["Type"].as_str().unwrap_or("").to_string();
        if kind == "loopback" {
            continue;
        }
        let addresses = i["Addresses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                let bytes: Vec<u8> = a["Address"]
                    .as_array()?
                    .iter()
                    .filter_map(|b| b.as_u64().map(|b| b as u8))
                    .collect();
                let prefix = a["PrefixLength"].as_u64().unwrap_or(0);
                match bytes.len() {
                    4 => Some(format!(
                        "{}/{prefix}",
                        std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3])
                    )),
                    16 => {
                        let arr: [u8; 16] = bytes.try_into().ok()?;
                        let ip = std::net::Ipv6Addr::from(arr);
                        // Link-local addresses are noise for people.
                        (!ip.is_unicast_link_local()).then(|| format!("{ip}/{prefix}"))
                    }
                    _ => None,
                }
            })
            .collect();
        out.push(Link {
            name: i["Name"].as_str().unwrap_or("").to_string(),
            kind,
            state: i["OperationalState"].as_str().unwrap_or("").to_string(),
            addresses,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_json() {
        let j = r#"{"Interfaces":[
            {"Index":1,"Name":"lo","Type":"loopback","OperationalState":"carrier"},
            {"Index":2,"Name":"end0","Type":"ether","OperationalState":"routable",
             "Addresses":[{"Family":2,"Address":[192,168,1,23],"PrefixLength":24},
                          {"Family":10,"Address":[254,128,0,0,0,0,0,0,0,0,0,0,0,0,0,1],"PrefixLength":64}]},
            {"Index":3,"Name":"wlan0","Type":"wlan","OperationalState":"no-carrier"}]}"#;
        let l = parse_describe(j);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].name, "end0");
        assert_eq!(l[0].addresses, ["192.168.1.23/24"]);
        assert_eq!(l[0].friendly_state(), "Connected");
        assert_eq!(l[1].friendly_state(), "Cable unplugged");
    }

    #[test]
    fn signal_bars() {
        let n = |dbm| Network {
            ssid: "x".into(),
            path: String::new(),
            security: Security::Psk,
            dbm,
            connected: false,
            known: false,
        };
        assert_eq!(n(-40).bars(), 4);
        assert_eq!(n(-70).bars(), 2);
        assert_eq!(n(-95).bars(), 0);
    }
}
