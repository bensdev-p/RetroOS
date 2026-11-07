//! Network settings kept in files (Control Panel → Network): the Wi-Fi
//! country, a fixed address for the wired network, and enterprise (802.1X)
//! Wi-Fi sign-in for iwd. Each is written by a root helper the admin group
//! may run (D-043, D-045).

use crate::admin::{self, AdminError};
use std::net::{IpAddr, Ipv4Addr};

/// Where `set-wifi-country` keeps the country (cfg80211's regulatory domain,
/// read when the Wi-Fi driver loads).
pub const COUNTRY_CONF: &str = "/etc/modprobe.d/retroos-wifi-country.conf";
/// The wired network's fixed address (sorts before 80-retroos-wired.network,
/// so networkd uses it instead of DHCP).
pub const WIRED_CONF: &str = "/etc/systemd/network/70-retroos-wired.network";

/// The Wi-Fi country ("GB"), or "" when unset (the world-wide default: only
/// the channels allowed everywhere).
pub fn country() -> String {
    parse_country(&std::fs::read_to_string(COUNTRY_CONF).unwrap_or_default())
}

pub fn parse_country(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find_map(|l| l.split("ieee80211_regdom=").nth(1))
        .map(|v| v.trim().to_ascii_uppercase())
        .filter(|v| v.len() == 2)
        .unwrap_or_default()
}

/// Every country as (code, name), by name (tzdata's iso3166.tab).
pub fn countries() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_to_string("/usr/share/zoneinfo/iso3166.tab")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let (c, n) = l.split_once('\t')?;
            Some((c.trim().to_string(), n.trim().to_string()))
        })
        .collect();
    if v.is_empty() {
        v = [
            ("GB", "Britain (UK)"),
            ("DE", "Germany"),
            ("US", "United States"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    }
    v.sort_by_key(|(_, n)| n.to_lowercase());
    v
}

/// Set the Wi-Fi country (two letters, or "" for none); applies from the
/// next start.
pub fn set_country(code: &str) -> Result<(), AdminError> {
    let code = code.to_ascii_uppercase();
    if !(code.is_empty() || (code.len() == 2 && code.chars().all(|c| c.is_ascii_uppercase()))) {
        return Err(AdminError::Failed("Invalid country code".into()));
    }
    admin::run_helper(
        "set-wifi-country",
        &[if code.is_empty() { "none" } else { &code }],
    )
}

/// How the wired network gets its address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wired {
    /// Automatic (DHCP); the rest is empty then.
    pub dhcp: bool,
    /// "192.168.1.50/24".
    pub address: String,
    pub gateway: String,
    /// Space-separated name servers.
    pub dns: String,
}

pub fn wired() -> Wired {
    parse_wired(&std::fs::read_to_string(WIRED_CONF).unwrap_or_default())
}

pub fn parse_wired(text: &str) -> Wired {
    let get = |k: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| {
                l.trim()
                    .strip_prefix(k)?
                    .strip_prefix('=')
                    .map(|v| v.trim().to_string())
            })
            .collect()
    };
    let address = get("Address").into_iter().next().unwrap_or_default();
    if address.is_empty() {
        return Wired {
            dhcp: true,
            ..Wired::default()
        };
    }
    Wired {
        dhcp: false,
        address,
        gateway: get("Gateway").into_iter().next().unwrap_or_default(),
        dns: get("DNS").join(" "),
    }
}

/// "a.b.c.d/n" with n 1–32; "/24" is added when the prefix is left out.
pub fn check_address(s: &str) -> Option<String> {
    let s = s.trim();
    let (ip, prefix) = match s.split_once('/') {
        Some((ip, p)) => (ip, p.parse::<u8>().ok()?),
        None => (s, 24),
    };
    let ip: Ipv4Addr = ip.parse().ok()?;
    (1..=32).contains(&prefix).then(|| format!("{ip}/{prefix}"))
}

impl Wired {
    /// Check a fixed configuration; the message says what is wrong.
    pub fn validate(&self) -> Result<Wired, String> {
        if self.dhcp {
            return Ok(Wired {
                dhcp: true,
                ..Wired::default()
            });
        }
        let address = check_address(&self.address)
            .ok_or("Type the IP address as four numbers, e.g. 192.168.1.50/24.")?;
        let gateway = self.gateway.trim().to_string();
        if !gateway.is_empty() && gateway.parse::<Ipv4Addr>().is_err() {
            return Err("The gateway is not an IP address.".into());
        }
        let dns: Vec<&str> = self
            .dns
            .split([' ', ','])
            .filter(|s| !s.is_empty())
            .collect();
        if dns.iter().any(|d| d.parse::<IpAddr>().is_err()) {
            return Err("Each name server must be an IP address.".into());
        }
        Ok(Wired {
            dhcp: false,
            address,
            gateway,
            dns: dns.join(" "),
        })
    }
}

/// Save the wired settings; networkd applies them at once.
pub fn set_wired(w: &Wired) -> Result<(), String> {
    let w = w.validate()?;
    let r = if w.dhcp {
        admin::run_helper("set-wired", &["dhcp"])
    } else {
        let mut args = vec!["static", w.address.as_str(), w.gateway.as_str()];
        args.extend(w.dns.split(' ').filter(|s| !s.is_empty()));
        admin::run_helper("set-wired", &args)
    };
    r.map_err(|e| e.to_string())
}

/// 802.1X methods offered for enterprise Wi-Fi (phase 2 is MSCHAPv2 for
/// PEAP, PAP for TTLS: what eduroam and most companies use).
pub const EAP_METHODS: &[(&str, &str)] = &[("PEAP", "PEAP"), ("TTLS", "TTLS")];

/// iwd's file name for a network: the SSID as is when it is plain letters,
/// digits, space, `-` and `_`, else `=` and its hex.
pub fn iwd_file_name(ssid: &str) -> String {
    let plain = ssid
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_');
    if plain {
        ssid.to_string()
    } else {
        let hex: String = ssid.bytes().map(|b| format!("{b:02x}")).collect();
        format!("={hex}")
    }
}

/// Teach iwd an enterprise network's sign-in; then connect as usual.
pub fn provision_enterprise(
    ssid: &str,
    method: &str,
    identity: &str,
    password: &str,
) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("The network name must be 1 to 32 characters.".into());
    }
    if identity.is_empty() || identity.contains(['\n', '\r']) || password.contains(['\n', '\r']) {
        return Err("Type your user name and password.".into());
    }
    if !EAP_METHODS.iter().any(|(m, _)| *m == method) {
        return Err("Unknown sign-in method.".into());
    }
    let name = iwd_file_name(ssid);
    admin::run_helper_input("set-wifi-enterprise", &[&name, method, identity], password)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_country_and_wired_files() {
        assert_eq!(
            parse_country("# RetroOS\noptions cfg80211 ieee80211_regdom=gb\n"),
            "GB"
        );
        assert_eq!(parse_country(""), "");
        let w = parse_wired("[Match]\nName=e*\n[Network]\nAddress=10.0.0.5/8\nGateway=10.0.0.1\nDNS=1.1.1.1\nDNS=9.9.9.9\n");
        assert_eq!(
            w,
            Wired {
                dhcp: false,
                address: "10.0.0.5/8".into(),
                gateway: "10.0.0.1".into(),
                dns: "1.1.1.1 9.9.9.9".into()
            }
        );
        assert!(parse_wired("").dhcp);
    }

    #[test]
    fn fixed_addresses_are_checked() {
        let w = |a: &str, g: &str, d: &str| Wired {
            dhcp: false,
            address: a.into(),
            gateway: g.into(),
            dns: d.into(),
        };
        assert_eq!(
            w("192.168.1.50", "", "").validate().unwrap().address,
            "192.168.1.50/24"
        );
        assert!(w("192.168.1.500/24", "", "").validate().is_err());
        assert!(w("10.0.0.2/33", "", "").validate().is_err());
        assert!(w("10.0.0.2/8", "10.0.0", "").validate().is_err());
        assert_eq!(
            w("10.0.0.2/8", "10.0.0.1", "1.1.1.1, 2606:4700::1111")
                .validate()
                .unwrap()
                .dns,
            "1.1.1.1 2606:4700::1111"
        );
        assert!(w("10.0.0.2/8", "", "one.one").validate().is_err());
    }

    #[test]
    fn iwd_names() {
        assert_eq!(iwd_file_name("eduroam"), "eduroam");
        assert_eq!(iwd_file_name("Café"), "=436166c3a9");
        assert_eq!(iwd_file_name("a.b"), "=612e62");
    }
}
