//! Wi-Fi on FreeBSD (ADR-0095): wpa_supplicant, through wpa_cli, for what
//! NetworkManager does on Linux. The supplicant is the one rc.conf starts
//! (`ifconfig_wlan0="WPA DHCP"`), and its control socket is open to `wheel`.
//!
//! Joining a new network saves it in /etc/wpa_supplicant.conf, which needs
//! `update_config=1` there; without it the network works until the supplicant
//! restarts.
//!
//! The parsing is plain functions on wpa_cli's output, tested on any system.

use super::{Network, Security, Wifi, WifiBackend, sort};

/// 0 to 100 from a signal level in dBm: -90 and below is nothing, -30 and
/// above is full.
fn strength(dbm: i32) -> u8 {
    ((dbm + 90) * 100 / 60).clamp(0, 100) as u8
}

/// How a network is secured, from the flags `scan_results` prints:
/// "[WPA2-PSK-CCMP][WPS][ESS]".
fn security(flags: &str) -> Security {
    if flags.contains("EAP") || flags.contains("WEP") {
        Security::Other
    } else if flags.contains("PSK") {
        Security::Psk
    } else if flags.contains("SAE") {
        Security::Sae
    } else if flags.contains("WPA") {
        Security::Other
    } else {
        Security::Open
    }
}

/// One line of `scan_results`.
#[derive(Debug, Clone, PartialEq)]
struct Scanned {
    bssid: String,
    ssid: String,
    dbm: i32,
    security: Security,
}

/// `scan_results`: a header, then bssid, frequency, signal, flags and ssid
/// by tabs. Hidden networks (no name, or NULs) are left out.
fn parse_scan(text: &str) -> Vec<Scanned> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let bssid = fields.next()?;
            let _frequency = fields.next()?;
            let dbm = fields.next()?.parse().ok()?;
            let flags = fields.next()?;
            let ssid = fields.next()?;
            (!ssid.is_empty() && !ssid.starts_with("\\x00")).then(|| Scanned {
                bssid: bssid.into(),
                ssid: ssid.into(),
                dbm,
                security: security(flags),
            })
        })
        .collect()
}

/// `list_networks`: a header, then id, ssid, bssid and flags by tabs. FreeBSD's
/// stock configuration has a nameless open entry, which isn't a network.
fn parse_saved(text: &str) -> Vec<(String, String)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let id = fields.next()?;
            let ssid = fields.next()?;
            (!ssid.is_empty()).then(|| (id.to_string(), ssid.to_string()))
        })
        .collect()
}

/// `status`: the network we're on, once the supplicant says it's up.
fn parse_status(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
    };
    (value("wpa_state")? == "COMPLETED")
        .then(|| value("ssid").map(str::to_string))
        .flatten()
}

/// Whether `ifconfig`'s first line has UP among its flags.
fn parse_up(text: &str) -> bool {
    text.lines()
        .next()
        .and_then(|l| l.split_once('<'))
        .and_then(|(_, rest)| rest.split_once('>'))
        .is_some_and(|(flags, _)| flags.split(',').any(|f| f == "UP"))
}

/// The scan's networks as the menu shows them: the strongest access point of
/// each name.
fn assemble(scan: &[Scanned], saved: &[(String, String)], active: Option<&str>) -> Vec<Network> {
    let mut networks: Vec<Network> = Vec::new();
    for s in scan {
        let network = Network {
            ssid: s.ssid.clone(),
            strength: strength(s.dbm),
            security: s.security,
            active: active == Some(s.ssid.as_str()),
            saved: saved.iter().any(|(_, ssid)| *ssid == s.ssid),
            access_point: s.bssid.clone(),
        };
        match networks.iter_mut().find(|n| n.ssid == s.ssid) {
            Some(other) if other.strength >= network.strength => {}
            Some(other) => *other = network,
            None => networks.push(network),
        }
    }
    sort(&mut networks);
    networks
}

fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

pub struct WpaSupplicant {
    interface: String,
}

impl WpaSupplicant {
    /// The machine's first wlan interface.
    pub fn new() -> Result<Self, String> {
        let out = run("ifconfig", &["-g", "wlan"])?;
        let interface = out.split_whitespace().next().ok_or("no Wi-Fi interface")?;
        Ok(Self {
            interface: interface.into(),
        })
    }

    /// wpa_cli on the interface; it prints FAIL, and exits well, when the
    /// supplicant refuses.
    fn cli(&self, args: &[&str]) -> Result<String, String> {
        let mut all = vec!["-i", self.interface.as_str()];
        all.extend_from_slice(args);
        let out = run("wpa_cli", &all)?;
        if out.trim() == "FAIL" {
            Err(format!("wpa_supplicant refused {}", args[0]))
        } else {
            Ok(out)
        }
    }

    fn up(&self) -> bool {
        run("ifconfig", &[&self.interface]).is_ok_and(|o| parse_up(&o))
    }

    /// The network we're on, if any.
    fn active(&self) -> Option<String> {
        parse_status(&self.cli(&["status"]).ok()?)
    }

    fn configure(&self, id: &str, network: &Network, password: Option<&str>) -> Result<(), String> {
        // The name as hex, so no quote or space in it needs care.
        self.cli(&["set_network", id, "ssid", &hex(&network.ssid)])?;
        match (network.security, password) {
            (Security::Psk, Some(pw)) => {
                self.cli(&["set_network", id, "psk", &format!("\"{pw}\"")])?;
            }
            (Security::Sae, Some(pw)) => {
                self.cli(&["set_network", id, "key_mgmt", "SAE"])?;
                self.cli(&["set_network", id, "sae_password", &format!("\"{pw}\"")])?;
                self.cli(&["set_network", id, "ieee80211w", "2"])?;
            }
            _ => {
                self.cli(&["set_network", id, "key_mgmt", "NONE"])?;
            }
        }
        self.cli(&["select_network", id])?;
        // Kept across restarts where the configuration allows it.
        let _ = self.cli(&["save_config"]);
        Ok(())
    }
}

impl WifiBackend for WpaSupplicant {
    /// Wi-Fi's state and the networks in range.
    fn wifi(&self) -> Result<Wifi, String> {
        let enabled = self.up();
        if !enabled {
            return Ok(Wifi {
                enabled,
                present: true,
                networks: Vec::new(),
            });
        }
        let scan = parse_scan(&self.cli(&["scan_results"])?);
        let saved = parse_saved(&self.cli(&["list_networks"])?);
        Ok(Wifi {
            enabled,
            present: true,
            networks: assemble(&scan, &saved, self.active().as_deref()),
        })
    }

    /// For the bar's icon, cheaply: `Some(None)` when it's off or not
    /// connected, else the signal.
    fn signal(&self) -> Option<Option<u8>> {
        if !self.up() {
            return Some(None);
        }
        let Some(active) = self.active() else {
            return Some(None);
        };
        let scan = parse_scan(&self.cli(&["scan_results"]).ok()?);
        Some(
            scan.iter()
                .filter(|s| s.ssid == active)
                .map(|s| strength(s.dbm))
                .max(),
        )
    }

    /// Asks for a new scan; the supplicant limits how often, so a refusal
    /// is fine.
    fn scan(&self) {
        let _ = self.cli(&["scan"]);
    }

    /// Joins `network`: with its saved entry if there is one, otherwise a new
    /// one, with `password` for a secured network.
    fn connect(&self, network: &Network, password: Option<&str>) -> Result<(), String> {
        let saved = parse_saved(&self.cli(&["list_networks"])?);
        if let Some((id, _)) = saved.iter().find(|(_, ssid)| *ssid == network.ssid) {
            return self.cli(&["select_network", id]).map(|_| ());
        }
        let password = match network.security {
            Security::Open => None,
            Security::Psk | Security::Sae => Some(password.ok_or("a password is needed")?),
            Security::Other => {
                return Err("this kind of network needs /etc/wpa_supplicant.conf".into());
            }
        };
        let id = self.cli(&["add_network"])?.trim().to_string();
        let result = self.configure(&id, network, password);
        if result.is_err() {
            let _ = self.cli(&["remove_network", &id]);
        }
        result
    }

    /// Leaves the current network.
    fn disconnect(&self) -> Result<(), String> {
        self.cli(&["disconnect"]).map(|_| ())
    }

    /// Wi-Fi on or off: the interface up or down, which takes root.
    fn set_enabled(&self, on: bool) -> Result<(), String> {
        run(
            "doas",
            &[
                "-n",
                "ifconfig",
                &self.interface,
                if on { "up" } else { "down" },
            ],
        )
        .map(|_| ())
    }
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program}: {}", err.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // What wpa_cli printed on loki.
    const SCAN: &str = "bssid / frequency / signal level / flags / ssid\n\
        b0:5b:99:00:fd:66\t2412\t-61\t[WPA2-PSK-CCMP][WPS][ESS]\tGNU\n\
        18:69:45:2c:97:7b\t5180\t-66\t[WPA2-PSK-CCMP][ESS]\tTP-Link_977B\n\
        aa:aa:aa:aa:aa:aa\t5180\t-80\t[WPA2-PSK-CCMP][ESS]\tGNU\n\
        bb:bb:bb:bb:bb:bb\t2437\t-50\t[ESS]\tCafe\n\
        cc:cc:cc:cc:cc:cc\t2437\t-40\t[WPA2-EAP-CCMP][ESS]\tOffice\n\
        dd:dd:dd:dd:dd:dd\t2437\t-40\t[WPA2-PSK-CCMP][ESS]\t\n";
    const SAVED: &str = "network id / ssid / bssid / flags\n0\tGNU\tany\t[CURRENT]\n1\t\tany\t\n";
    const STATUS: &str = "bssid=b0:5b:99:00:fd:66\nfreq=0\nssid=GNU\nid=0\nwpa_state=COMPLETED\n";

    #[test]
    fn signal_levels_in_percent() {
        assert_eq!(strength(-90), 0);
        assert_eq!(strength(-120), 0);
        assert_eq!(strength(-60), 50);
        assert_eq!(strength(-30), 100);
        assert_eq!(strength(-10), 100);
    }

    #[test]
    fn security_from_flags() {
        assert_eq!(security("[ESS]"), Security::Open);
        assert_eq!(security("[WPA2-PSK-CCMP][WPS][ESS]"), Security::Psk);
        assert_eq!(security("[WPA2-PSK-CCMP+SAE][ESS]"), Security::Psk);
        assert_eq!(security("[RSN-SAE-CCMP][ESS]"), Security::Sae);
        assert_eq!(security("[WPA2-EAP-CCMP][ESS]"), Security::Other);
        assert_eq!(security("[WEP][ESS]"), Security::Other);
    }

    #[test]
    fn the_scan_without_hidden_networks() {
        let scan = parse_scan(SCAN);
        assert_eq!(scan.len(), 5);
        assert_eq!(scan[0].bssid, "b0:5b:99:00:fd:66");
        assert_eq!(scan[0].dbm, -61);
    }

    #[test]
    fn saved_networks_leave_out_the_nameless_one() {
        assert_eq!(parse_saved(SAVED), [("0".to_string(), "GNU".to_string())]);
    }

    #[test]
    fn status_names_the_network_only_once_up() {
        assert_eq!(parse_status(STATUS).as_deref(), Some("GNU"));
        assert_eq!(
            parse_status("ssid=GNU\nwpa_state=SCANNING\n"),
            None,
            "not connected yet"
        );
        assert_eq!(parse_status("wpa_state=DISCONNECTED\n"), None);
    }

    #[test]
    fn the_interface_up_or_down() {
        assert!(parse_up(
            "wlan0: flags=8843<UP,BROADCAST,RUNNING> metric 0 mtu 1500\n\tether x\n"
        ));
        assert!(!parse_up(
            "wlan0: flags=8802<BROADCAST,SIMPLEX> metric 0 mtu 1500\n"
        ));
        assert!(!parse_up(""));
    }

    #[test]
    fn active_then_saved_one_row_per_name_the_strongest_point() {
        let networks = assemble(&parse_scan(SCAN), &parse_saved(SAVED), Some("GNU"));
        let names: Vec<&str> = networks.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(names, ["GNU", "Office", "Cafe", "TP-Link_977B"]);
        let gnu = &networks[0];
        assert!(gnu.active && gnu.saved);
        assert_eq!(gnu.strength, strength(-61));
        assert_eq!(gnu.access_point, "b0:5b:99:00:fd:66");
    }

    #[test]
    fn names_as_hex() {
        assert_eq!(hex("GNU 5"), "474e552035");
    }
}
