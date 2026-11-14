//! Whether a fullscreen window has the keyboard, as the compositor says on
//! `com.sectorflabs.Patina.Input` (ADR-0129): its `Fullscreen` property now, and
//! `FullscreenChanged` after. It's a fact about the desktop, so nothing here
//! depends on Night Light's settings.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use patina_shell::calloop::channel::Sender;

const NAME: &str = "com.sectorflabs.Patina.Input";
const PATH: &str = "/com/sectorflabs/Patina/Input";

/// Follows the compositor on a thread of its own, keeping `flag` true while a
/// fullscreen window has the keyboard and poking `wake` at each change. Where
/// the compositor doesn't serve it (nested, or another desktop) it stays false.
pub fn watch(flag: Arc<AtomicBool>, wake: Sender<()>) {
    let spawned = std::thread::Builder::new()
        .name("fullscreen".into())
        .spawn(move || {
            if let Err(e) = follow(&flag, &wake) {
                eprintln!("patina-notify: fullscreen windows aren't followed: {e}");
            }
        });
    if let Err(e) = spawned {
        eprintln!("patina-notify: fullscreen windows aren't followed: {e}");
    }
}

fn follow(flag: &AtomicBool, wake: &Sender<()>) -> Result<(), String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    // Subscribe before asking, so a change in between isn't missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(NAME)
        .and_then(|b| b.path(PATH))
        .map_err(|e| e.to_string())?
        .build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None)
        .map_err(|e| e.to_string())?;
    if now(&bus) {
        flag.store(true, Ordering::Relaxed);
        let _ = wake.send(());
    }
    for message in signals.flatten() {
        if message.header().member().map(|m| m.as_str()) == Some("FullscreenChanged")
            && let Ok(on) = message.body().deserialize::<bool>()
        {
            flag.store(on, Ordering::Relaxed);
            let _ = wake.send(());
        }
    }
    Err("the session bus went away".into())
}

/// The compositor's `Fullscreen` property; false if it doesn't say.
fn now(bus: &zbus::blocking::Connection) -> bool {
    bus.call_method(
        Some(NAME),
        PATH,
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &(NAME, "Fullscreen"),
    )
    .ok()
    .and_then(|reply| {
        reply
            .body()
            .deserialize::<zbus::zvariant::Value>()
            .ok()
            .and_then(|v| bool::try_from(v).ok())
    })
    .unwrap_or(false)
}
