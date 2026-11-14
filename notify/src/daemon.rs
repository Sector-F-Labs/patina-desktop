//! `org.freedesktop.Notifications` on the session bus (ADR-0141). The calls
//! land in the shared store and wake the column's loop; what the column does
//! (a card dismissed, an action clicked) goes back out as the signals.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use patina_shell::calloop::channel::Sender;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use crate::store::{self, Note, Store, Urgency};

pub const NAME: &str = "org.freedesktop.Notifications";
pub const PATH: &str = "/org/freedesktop/Notifications";

/// What the bus thread and the column share.
pub struct Shared {
    pub store: Mutex<Store>,
    pub enabled: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            store: Mutex::new(Store::default()),
            enabled: AtomicBool::new(true),
        })
    }

    pub fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct Interface {
    shared: Arc<Shared>,
    wake: Sender<()>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Interface {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["body", "body-markup", "actions"]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        _app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let urgency = hints
            .get("urgency")
            .and_then(|v| v.downcast_ref::<u8>().ok());
        let desktop_entry = hints
            .get("desktop-entry")
            .and_then(|v| v.downcast_ref::<&str>().ok())
            .unwrap_or_default()
            .trim_end_matches(".desktop")
            .to_string();
        let note = Note {
            id: 0,
            app: app_name,
            desktop_entry,
            summary,
            body: store::plain(&body),
            actions: store::action_pairs(&actions),
            urgency: Urgency::from_hint(urgency),
            expires: None,
        };
        let mut notes = self.shared.store();
        let id = notes.notify(replaces_id, note, expire_timeout, Instant::now());
        // Switched off: the app still gets an answer, and nothing shows.
        if self.shared.enabled.load(Ordering::Relaxed) {
            let _ = self.wake.send(());
        } else {
            notes.close(id);
        }
        id
    }

    async fn close_notification(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        id: u32,
    ) -> zbus::fdo::Result<()> {
        let closed = self.shared.store().close(id).is_some();
        if closed {
            let _ = self.wake.send(());
            Self::notification_closed(&emitter, id, store::CLOSED_BY_CALL)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(())
    }

    fn get_server_information(&self) -> (String, String, String, String) {
        (
            "patina-notify".into(),
            "Sector F Labs".into(),
            env!("CARGO_PKG_VERSION").into(),
            "1.2".into(),
        )
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: String,
    ) -> zbus::Result<()>;
}

/// The connection that owns the name; the column tells apps what became of
/// their notifications through it.
pub struct Daemon {
    conn: zbus::blocking::Connection,
}

impl Daemon {
    /// Claims the name and starts serving. `wake` is poked whenever the store
    /// changes from the bus's side.
    pub fn serve(shared: Arc<Shared>, wake: Sender<()>) -> Result<Daemon, String> {
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .and_then(|b| b.serve_at(PATH, Interface { shared, wake }))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        Ok(Daemon { conn })
    }

    pub fn closed(&self, id: u32, reason: u32) {
        let _ = self.conn.emit_signal(
            None::<&str>,
            PATH,
            NAME,
            "NotificationClosed",
            &(id, reason),
        );
    }

    pub fn action(&self, id: u32, key: &str) {
        let _ = self
            .conn
            .emit_signal(None::<&str>, PATH, NAME, "ActionInvoked", &(id, key));
    }
}
