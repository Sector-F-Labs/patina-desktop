//! Caffeine: holding the screen on from the bar's tray, as any app that
//! plays a video does, through `org.freedesktop.ScreenSaver`'s Inhibit on
//! the session bus (the compositor's `idle/dbus.rs`, or the parent desktop's
//! when nested). The compositor drops what a name held when it leaves the
//! bus, so a bar that quits or crashes never leaves the screen held.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use zbus::blocking::{Connection, Proxy};

const NAME: &str = "org.freedesktop.ScreenSaver";
const PATH: &str = "/org/freedesktop/ScreenSaver";

/// A hold on the screen: on or off, shared by clones.
#[derive(Clone, Default)]
pub struct Caffeine {
    on: Arc<AtomicBool>,
    /// The connection the Inhibit was made on, and its cookie.
    held: Arc<Mutex<Option<(Connection, u32)>>>,
}

impl Caffeine {
    /// Whether the screen is being held on.
    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    /// Holds the screen on, or lets go, off the caller's thread (the
    /// service may take a moment to answer). `is_on` changes at once, so
    /// the next frame shows it, and goes back if the service refuses.
    pub fn toggle(&self) {
        let me = self.clone();
        self.on.fetch_xor(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            if let Err(e) = me.flip() {
                eprintln!("patina-system: caffeine: {e}");
                let held = me.held.lock().is_ok_and(|h| h.is_some());
                me.on.store(held, Ordering::Relaxed);
            }
        });
    }

    fn flip(&self) -> Result<(), String> {
        let mut held = self.held.lock().map_err(|e| e.to_string())?;
        if let Some((connection, cookie)) = held.take() {
            self.on.store(false, Ordering::Relaxed);
            return proxy(&connection)?
                .call::<_, _, ()>("UnInhibit", &(cookie,))
                .map_err(|e| e.to_string());
        }
        // A connection of its own: the hold is the name's, and goes with it.
        let connection = Connection::session().map_err(|e| e.to_string())?;
        let cookie: u32 = proxy(&connection)?
            .call("Inhibit", &("Patina", "Caffeine"))
            .map_err(|e| e.to_string())?;
        *held = Some((connection, cookie));
        self.on.store(true, Ordering::Relaxed);
        Ok(())
    }
}

fn proxy(connection: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(connection, NAME, PATH, NAME).map_err(|e| e.to_string())
}
