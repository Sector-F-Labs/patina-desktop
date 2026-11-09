//! Typing on the session bus (ADR-0071, ADR-0072), for dictation:
//! `com.sectorflabs.Patina.Input` at `/com/sectorflabs/Patina/Input`, with
//! `Commit(text)`, which types `text` into the focused window, and the
//! signals `DictationStarted` and `DictationStopped`, as the dictate key goes
//! down and comes up. Also `FullscreenChanged(b)` and the property `Fullscreen`:
//! whether a fullscreen window has the keyboard, a game or a video (ADR-0129),
//! so that what won't be wanted meanwhile can be put away. Served nested too.
//!
//! zbus runs on threads of its own; texts reach the event loop over a
//! calloop channel, and the signals leave it over another to a thread that
//! emits them, so the loop never waits on the bus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Input";
pub const PATH: &str = "/com/sectorflabs/Patina/Input";

/// The compositor's end, served while it lives.
pub struct Service {
    _connection: zbus::blocking::Connection,
    announced: mpsc::Sender<Announcement>,
    /// What the `Fullscreen` property says.
    fullscreen: Arc<AtomicBool>,
}

/// What listeners are told.
enum Announcement {
    /// The dictate key went down (true) or came up (false).
    Dictation(bool),
    /// A fullscreen window has the keyboard (true) or no longer does.
    Fullscreen(bool),
}

impl Service {
    /// Tells listeners the dictate key went down (`started`) or came up.
    pub fn announce(&self, started: bool) {
        let _ = self.announced.send(Announcement::Dictation(started));
    }

    /// Tells listeners a fullscreen window has the keyboard, or no longer has.
    pub fn announce_fullscreen(&self, fullscreen: bool) {
        self.fullscreen.store(fullscreen, Ordering::Relaxed);
        let _ = self.announced.send(Announcement::Fullscreen(fullscreen));
    }
}

/// Claims the name and serves the interface; texts go to the loop at
/// `handle`.
pub fn serve(handle: &LoopHandle<'static, State>) -> Result<Service, String> {
    let (texts, incoming) = channel::channel::<String>();
    handle
        .insert_source(incoming, |event, _, state| {
            if let channel::Event::Msg(text) = event {
                state.commit_text(text);
            }
        })
        .map_err(|e| e.to_string())?;
    serve_with(texts)
}

fn serve_with(texts: channel::Sender<String>) -> Result<Service, String> {
    let fullscreen = Arc::new(AtomicBool::new(false));
    let connection = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| {
            b.serve_at(
                PATH,
                Interface {
                    texts,
                    fullscreen: fullscreen.clone(),
                },
            )
        })
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?;
    let (dictation, announced) = mpsc::channel();
    let emitting = connection.clone();
    std::thread::Builder::new()
        .name("typing".into())
        .spawn(move || announce(&emitting, &announced))
        .map_err(|e| e.to_string())?;
    Ok(Service {
        _connection: connection,
        announced: dictation,
        fullscreen,
    })
}

/// Emits the signals as the compositor sends them.
fn announce(connection: &zbus::blocking::Connection, announced: &mpsc::Receiver<Announcement>) {
    let Ok(interface) = connection.object_server().interface::<_, Interface>(PATH) else {
        return;
    };
    while let Ok(announcement) = announced.recv() {
        let emitter = interface.signal_emitter();
        let result = zbus::block_on(async {
            match announcement {
                Announcement::Dictation(true) => Interface::dictation_started(emitter).await,
                Announcement::Dictation(false) => Interface::dictation_stopped(emitter).await,
                Announcement::Fullscreen(on) => Interface::fullscreen_changed(emitter, on).await,
            }
        });
        if let Err(e) = result {
            eprintln!("patina-compositor: typing: {e}");
        }
    }
}

struct Interface {
    texts: channel::Sender<String>,
    fullscreen: Arc<AtomicBool>,
}

#[zbus::interface(name = "com.sectorflabs.Patina.Input")]
impl Interface {
    /// Types `text` into the focused window, through the clipboard, which
    /// is given back as it was after.
    fn commit(&self, text: String) -> zbus::fdo::Result<()> {
        self.texts
            .send(text)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// The dictate key went down: record from now.
    #[zbus(signal)]
    async fn dictation_started(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;

    /// The dictate key came up: what was said can be committed.
    #[zbus(signal)]
    async fn dictation_stopped(
        emitter: &zbus::object_server::SignalEmitter<'_>,
    ) -> zbus::Result<()>;

    /// A fullscreen window, a game or a video, gained the keyboard (true) or
    /// lost it, or closed, or the screen locked (false).
    #[zbus(signal)]
    async fn fullscreen_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        fullscreen: bool,
    ) -> zbus::Result<()>;

    /// Whether a fullscreen window has the keyboard now, for what starts
    /// after the signal was sent.
    #[zbus(property(emits_changed_signal = "false"))]
    fn fullscreen(&self) -> bool {
        self.fullscreen.load(Ordering::Relaxed)
    }
}

/// On a bus of its own: `dbus-run-session -- cargo test -p
/// patina-compositor typing -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn texts_reach_the_loop_and_the_key_reaches_listeners() {
        let mut event_loop: EventLoop<'static, Vec<String>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<String>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, got| {
                if let channel::Event::Msg(text) = event {
                    got.push(text);
                }
            })
            .unwrap();
        let service = serve_with(send).expect("the name is free on a bus of our own");
        let bus = zbus::blocking::Connection::session().unwrap();
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(NAME)
            .unwrap()
            .build();
        let mut signals =
            zbus::blocking::MessageIterator::for_match_rule(rule, &bus, None).unwrap();

        bus.call_method(Some(NAME), PATH, Some(NAME), "Commit", &("hej då",))
            .unwrap();
        let mut got = Vec::new();
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(20)), &mut got)
                .unwrap();
        }
        assert_eq!(got, ["hej då"]);

        service.announce(true);
        service.announce(false);
        let member = |m: zbus::Result<zbus::Message>| {
            m.unwrap().header().member().map(|m| m.to_string()).unwrap()
        };
        assert_eq!(member(signals.next().unwrap()), "DictationStarted");
        assert_eq!(member(signals.next().unwrap()), "DictationStopped");

        // Fullscreen: the signal, and the property for a listener that comes late.
        let fullscreen = || -> bool {
            bus.call_method(
                Some(NAME),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(NAME, "Fullscreen"),
            )
            .unwrap()
            .body()
            .deserialize::<zbus::zvariant::Value>()
            .map(|v| bool::try_from(v).unwrap())
            .unwrap()
        };
        assert!(!fullscreen());
        service.announce_fullscreen(true);
        assert_eq!(member(signals.next().unwrap()), "FullscreenChanged");
        assert!(fullscreen());
        service.announce_fullscreen(false);
        assert_eq!(member(signals.next().unwrap()), "FullscreenChanged");
        assert!(!fullscreen());
    }
}
