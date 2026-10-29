//! Prints what the Wi-Fi menu reads (NetworkManager, or wpa_supplicant on FreeBSD), for checking the
//! D-Bus side on a machine: `cargo run -p patina-shell --example wifi`.

fn main() {
    let nm = match patina_system::network::open() {
        Ok(nm) => nm,
        Err(e) => return eprintln!("no Wi-Fi: {e}"),
    };
    match nm.wifi() {
        Ok(wifi) => {
            println!(
                "enabled {} present {} signal {:?}",
                wifi.enabled,
                wifi.present,
                wifi.signal()
            );
            for n in &wifi.networks {
                println!(
                    "{:3}% {:<6} {:<6} {:<5} {}",
                    n.strength,
                    if n.active { "active" } else { "" },
                    if n.saved { "saved" } else { "" },
                    format!("{:?}", n.security),
                    n.ssid
                );
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}
