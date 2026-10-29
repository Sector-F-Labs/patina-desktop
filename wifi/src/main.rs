//! patina-wifi: the Wi-Fi menu of the Patina desktop's bar (ADR-0022).
//! The bar's Wi-Fi icon runs it; running it again closes it.

mod ui;

use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece::{self, Single};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options};
use patina_system::network::{self, Wifi};

use crate::ui::{Command, Status, WifiMenu};

const USAGE: &str = "usage: patina-wifi

Opens the Wi-Fi menu at the bar's right end, or closes it if it's open.
PATINA_SKIN and PATINA_BAR_EDGE, as patina-bar sets them, choose the skin
and which edge it opens from.";

const SIZE: (u32, u32) = (360, 440);
const GAP: i32 = 4;
/// How often the networks are read again while the menu is open.
const REFRESH: Duration = Duration::from_secs(2);
/// How long to wait for a network to come up before saying it didn't.
const JOIN_TIMEOUT: Duration = Duration::from_secs(25);

enum Message {
    Wifi(Wifi),
    Status(Option<Status>),
    Close,
}

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-wifi {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-wifi: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let (send, receive) = channel::channel::<Message>();
    let closer = send.clone();
    let Some(_single) = Single::start("patina-wifi", "close", move |word| {
        word != "close" || closer.send(Message::Close).is_ok()
    })?
    else {
        return Ok(());
    };

    let (commands, received) = mpsc::channel::<Command>();
    std::thread::Builder::new()
        .name("networkmanager".into())
        .spawn(move || worker(&send, &received))
        .map_err(|e| e.to_string())?;

    let menu = WifiMenu::new(piece::skin("patina-wifi"), commands);
    // At the button that opened it, or on the bar's edge (ADR-0063).
    let (anchor, margin, exclusive_zone) = piece::opening(SIZE, GAP, Anchor::RIGHT);
    let options = Options {
        namespace: "patina-wifi".into(),
        layer: Layer::Overlay,
        anchor,
        size: SIZE,
        exclusive_zone,
        margin,
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, menu)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| {
            match event {
                Event::Msg(Message::Wifi(wifi)) => {
                    if host.shell.wifi.as_ref() == Some(&wifi) {
                        return;
                    }
                    host.shell.wifi = Some(wifi);
                }
                Event::Msg(Message::Status(status)) => host.shell.status = status,
                Event::Msg(Message::Close) => host.shell.close(),
                Event::Closed => return,
            }
            host.redraw_all();
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

/// Talks to NetworkManager (or wpa_supplicant): scans once, then refreshes every few seconds
/// and carries out the menu's commands, reporting progress.
fn worker(send: &channel::Sender<Message>, commands: &mpsc::Receiver<Command>) {
    let nm = match network::open() {
        Ok(nm) => nm,
        Err(e) => {
            let _ = send.send(Message::Status(Some(Status::Problem(format!(
                "Can't reach Wi-Fi: {e}"
            )))));
            return;
        }
    };
    nm.scan();
    // The network being joined, and since when.
    let mut joining: Option<(String, Instant)> = None;
    loop {
        match nm.wifi() {
            Ok(wifi) => {
                if let Some((ssid, since)) = &joining {
                    let joined = wifi.networks.iter().any(|n| n.active && &n.ssid == ssid);
                    if joined {
                        joining = None;
                        let _ = send.send(Message::Status(None));
                    } else if since.elapsed() > JOIN_TIMEOUT {
                        let _ = send.send(Message::Status(Some(Status::Problem(format!(
                            "Couldn't join {ssid}. Check the password and try again."
                        )))));
                        joining = None;
                    }
                }
                if send.send(Message::Wifi(wifi)).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = send.send(Message::Status(Some(Status::Problem(e))));
            }
        }
        let command = match commands.recv_timeout(REFRESH) {
            Ok(command) => command,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let result = match command {
            Command::Connect(network, password) => {
                joining = Some((network.ssid.clone(), Instant::now()));
                nm.connect(&network, password.as_deref())
            }
            Command::Disconnect => nm.disconnect(),
            Command::SetEnabled(on) => {
                let result = nm.set_enabled(on);
                if on {
                    nm.scan();
                }
                result
            }
        };
        if let Err(e) = result {
            joining = None;
            let _ = send.send(Message::Status(Some(Status::Problem(e))));
        }
    }
}
