//! Tiling on the session bus (ADR-0049): `com.sectorflabs.Patina.Tiling` at
//! `/com/sectorflabs/Patina/Tiling`.
//!
//! Signals say what the pointer and keys do: `DragStarted(id)`,
//! `DragMotion(id, x, y)`, `DragEnded(id, x, y)` with the pointer in the
//! desktop's logical pixels, `SnapRequested(id, direction)` for Super with
//! an arrow, `WorkAreaChanged(x, y, w, h)`, and `Resized(id, x, y, w, h)`
//! for each tiled window after the line between tiled windows is dragged.
//! Methods: `Windows()`, `WorkArea()`, `Gap()`, `Screen()`,
//! `Place(id, x, y, w, h)`,
//! `Restore(id)`, `ShowPreview(x, y, w, h)` and `HidePreview()`.
//!
//! Whoever owns `com.sectorflabs.Patina.Tiler` decides; the bus's own name
//! rules say who that is (`--replace`, queueing), and it stops deciding when
//! it leaves the bus. zbus runs on threads of its own: requests reach the
//! event loop over a calloop channel, and signals leave from a thread fed
//! by the loop, with only the latest of a run of motions sent.

use std::sync::mpsc;
use std::time::Duration;

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Tiling";
pub const PATH: &str = "/com/sectorflabs/Patina/Tiling";
/// The name a tiler owns to decide.
pub const TILER: &str = "com.sectorflabs.Patina.Tiler";

/// x, y, width and height, in logical pixels.
pub type Rect = (i32, i32, i32, i32);

/// A window: its id, app id, title, placement ("", "maximized", "fill",
/// "left", "right" or "rect") and where it is.
pub type WindowRow = (u64, String, String, String, Rect);

type Reply<T> = mpsc::Sender<T>;

/// What the bus asks of the compositor.
#[derive(Debug)]
pub enum Request {
    /// Whether an app owns the Tiler name.
    Tiler(bool),
    Windows(Reply<Vec<WindowRow>>),
    WorkArea(Reply<Rect>),
    Gap(Reply<i32>),
    Screen(Reply<Rect>),
    Place {
        id: u64,
        area: Rect,
        reply: Reply<Result<(), String>>,
    },
    Restore {
        id: u64,
        reply: Reply<Result<(), String>>,
    },
    Preview(Option<Rect>),
}

/// What the compositor tells the bus.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    DragStarted(u64),
    DragMotion(u64, f64, f64),
    DragEnded(u64, f64, f64),
    Snap(u64, &'static str),
    WorkArea(Rect),
    /// A tiled window, after the line between it and its neighbour was
    /// dragged (ADR-0054).
    Resized(u64, Rect),
}

/// The compositor's end: where its events go.
pub struct Service {
    events: mpsc::Sender<Event>,
}

impl Service {
    /// Claims the name and serves the interface, on threads of its own.
    /// Requests come into the loop at `handle`.
    pub fn start(handle: &LoopHandle<'static, State>) -> Result<Self, String> {
        let (requests, incoming) = channel::channel::<Request>();
        handle
            .insert_source(incoming, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    state.tiling_request(request);
                }
            })
            .map_err(|e| e.to_string())?;
        Self::serve(requests)
    }

    fn serve(requests: channel::Sender<Request>) -> Result<Self, String> {
        let interface = Interface {
            requests: requests.clone(),
        };
        let connection = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name(NAME))
            .and_then(|b| b.serve_at(PATH, interface))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())?;
        let watched = connection.clone();
        std::thread::Builder::new()
            .name("tiler-owner".into())
            .spawn(move || watch_tiler(&watched, &requests))
            .map_err(|e| e.to_string())?;
        let (events, outgoing) = mpsc::channel();
        std::thread::Builder::new()
            .name("tiling".into())
            .spawn(move || send_events(&connection, &outgoing))
            .map_err(|e| e.to_string())?;
        Ok(Self { events })
    }

    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

/// Tells the loop whether the Tiler name has an owner, now and whenever
/// that changes.
fn watch_tiler(connection: &zbus::blocking::Connection, requests: &channel::Sender<Request>) {
    let watch = zbus::blocking::fdo::DBusProxy::new(connection).and_then(|bus| {
        // Listening first, so an owner arriving between the two is heard.
        let changes = bus.receive_name_owner_changed()?;
        let owned = bus.name_has_owner(TILER.try_into()?)?;
        Ok((changes, owned))
    });
    let (changes, owned) = match watch {
        Ok(w) => w,
        Err(e) => {
            eprintln!("patina-compositor: tiling can't watch for a tiler: {e}");
            return;
        }
    };
    if requests.send(Request::Tiler(owned)).is_err() {
        return;
    }
    for signal in changes {
        let Ok(args) = signal.args() else { continue };
        if args.name() == TILER
            && requests
                .send(Request::Tiler(args.new_owner().is_some()))
                .is_err()
        {
            return;
        }
    }
}

/// Of the events waiting, those worth sending: a run of motions of the same
/// window is only its last.
fn coalesce(events: Vec<Event>) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::with_capacity(events.len());
    for event in events {
        if let (Some(Event::DragMotion(last, ..)), Event::DragMotion(id, ..)) = (out.last(), &event)
            && last == id
        {
            out.pop();
        }
        out.push(event);
    }
    out
}

fn send_events(connection: &zbus::blocking::Connection, events: &mpsc::Receiver<Event>) {
    let Ok(interface) = connection.object_server().interface::<_, Interface>(PATH) else {
        return;
    };
    while let Ok(first) = events.recv() {
        let waiting = std::iter::once(first).chain(events.try_iter()).collect();
        let emitter = interface.signal_emitter();
        for event in coalesce(waiting) {
            let result = zbus::block_on(async {
                match event {
                    Event::DragStarted(id) => Interface::drag_started(emitter, id).await,
                    Event::DragMotion(id, x, y) => Interface::drag_motion(emitter, id, x, y).await,
                    Event::DragEnded(id, x, y) => Interface::drag_ended(emitter, id, x, y).await,
                    Event::Snap(id, direction) => {
                        Interface::snap_requested(emitter, id, direction).await
                    }
                    Event::WorkArea((x, y, w, h)) => {
                        Interface::work_area_changed(emitter, x, y, w, h).await
                    }
                    Event::Resized(id, (x, y, w, h)) => {
                        Interface::resized(emitter, id, x, y, w, h).await
                    }
                }
            });
            if let Err(e) = result {
                eprintln!("patina-compositor: tiling: {e}");
            }
        }
    }
}

struct Interface {
    requests: channel::Sender<Request>,
}

fn failed(e: impl ToString) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}

impl Interface {
    /// Asks the loop, and waits a moment for its answer.
    fn ask<T>(&self, request: impl FnOnce(Reply<T>) -> Request) -> zbus::fdo::Result<T> {
        let (reply, answer) = mpsc::channel();
        self.requests.send(request(reply)).map_err(failed)?;
        answer
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| failed("the compositor didn't answer"))
    }
}

#[zbus::interface(name = "com.sectorflabs.Patina.Tiling")]
impl Interface {
    /// The open windows, bottom to top: (id, app id, title, placement,
    /// (x, y, width, height)). X11 windows aren't listed yet.
    fn windows(&self) -> zbus::fdo::Result<Vec<WindowRow>> {
        self.ask(Request::Windows)
    }

    /// The part of the screen windows tile in: all but the bar, less the
    /// gaps around it when they're on (ADR-0051).
    fn work_area(&self) -> zbus::fdo::Result<Rect> {
        self.ask(Request::WorkArea)
    }

    /// The gap to leave between tiled windows, in logical pixels; 0 when
    /// gaps are off. `WorkAreaChanged` follows a change.
    fn gap(&self) -> zbus::fdo::Result<i32> {
        self.ask(Request::Gap)
    }

    /// The whole screen, bar and all, for where the pointer reaches its
    /// edges.
    fn screen(&self) -> zbus::fdo::Result<Rect> {
        self.ask(Request::Screen)
    }

    /// Tiles window `id` at this rectangle; all of the work area fills it,
    /// tiled, not maximised. Dragging it, or `Restore`, puts it back as it
    /// was.
    fn place(&self, id: u64, x: i32, y: i32, width: i32, height: i32) -> zbus::fdo::Result<()> {
        self.ask(|reply| Request::Place {
            id,
            area: (x, y, width, height),
            reply,
        })?
        .map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// Puts window `id` back where and how big it was before it was tiled.
    fn restore(&self, id: u64) -> zbus::fdo::Result<()> {
        self.ask(|reply| Request::Restore { id, reply })?
            .map_err(zbus::fdo::Error::InvalidArgs)
    }

    /// Shows where a drop would go. The compositor hides it when a drag
    /// ends.
    fn show_preview(&self, x: i32, y: i32, width: i32, height: i32) -> zbus::fdo::Result<()> {
        self.requests
            .send(Request::Preview(Some((x, y, width, height))))
            .map_err(failed)
    }

    fn hide_preview(&self) -> zbus::fdo::Result<()> {
        self.requests.send(Request::Preview(None)).map_err(failed)
    }

    #[zbus(signal)]
    async fn drag_started(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn drag_motion(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
        x: f64,
        y: f64,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn drag_ended(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
        x: f64,
        y: f64,
    ) -> zbus::Result<()>;

    /// Super with an arrow on window `id`: "up", "down", "left" or "right".
    #[zbus(signal)]
    async fn snap_requested(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
        direction: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn work_area_changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> zbus::Result<()>;

    /// Tiled window `id` is now at this rectangle: the line between it and
    /// its neighbour was dragged.
    #[zbus(signal)]
    async fn resized(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> zbus::Result<()>;
}

/// The bus tests on a bus of their own: `dbus-run-session -- cargo test -p
/// patina-compositor tiling -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::Instant;

    use smithay::reexports::calloop::EventLoop;

    use super::*;

    /// Runs `event_loop` for `wait`, answering requests as a compositor
    /// with one window, 7, would, and returns them.
    fn answer(event_loop: &mut EventLoop<'static, Vec<String>>, wait: Duration) -> Vec<String> {
        let mut got = Vec::new();
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(10)), &mut got)
                .unwrap();
        }
        got
    }

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn a_tiler_claims_places_and_hears_drags() {
        use patina_system::tiling as client;

        let mut event_loop: EventLoop<'static, Vec<String>> = EventLoop::try_new().unwrap();
        let (send, incoming) = channel::channel::<Request>();
        event_loop
            .handle()
            .insert_source(incoming, |event, _, got| {
                let channel::Event::Msg(request) = event else {
                    return;
                };
                match request {
                    Request::Tiler(owned) => got.push(format!("tiler {owned}")),
                    Request::WorkArea(reply) => {
                        let _ = reply.send((0, 40, 1200, 760));
                    }
                    Request::Gap(reply) => {
                        let _ = reply.send(8);
                    }
                    Request::Screen(reply) => {
                        let _ = reply.send((0, 0, 1200, 800));
                    }
                    Request::Place { id, area, reply } => {
                        got.push(format!("place {id} {area:?}"));
                        let _ = reply.send(if id == 7 {
                            Ok(())
                        } else {
                            Err(format!("no window {id}"))
                        });
                    }
                    Request::Preview(area) => got.push(format!("preview {area:?}")),
                    other => got.push(format!("{other:?}")),
                }
            })
            .unwrap();
        let service = Service::serve(send).expect("the name is free on a bus of our own");
        assert_eq!(
            answer(&mut event_loop, Duration::from_millis(200)),
            ["tiler false"]
        );

        let bus = zbus::blocking::Connection::session().unwrap();
        let tiling = client::Tiling::new(&bus).unwrap();
        let mut events = tiling.events().unwrap();
        client::claim(&bus).unwrap();
        assert_eq!(
            answer(&mut event_loop, Duration::from_millis(200)),
            ["tiler true"]
        );

        // Calls block on the loop, so they're made from a thread of their own.
        let other = bus.clone();
        let caller = std::thread::spawn(move || {
            let tiling = client::Tiling::new(&other).unwrap();
            assert_eq!(tiling.work_area().unwrap(), (0, 40, 1200, 760));
            assert_eq!(tiling.gap().unwrap(), 8);
            assert_eq!(tiling.screen().unwrap(), (0, 0, 1200, 800));
            tiling.show_preview((0, 40, 400, 760)).unwrap();
            tiling.place(7, (0, 40, 400, 760)).unwrap();
            assert!(tiling.place(8, (0, 40, 400, 760)).is_err());
        });
        let got = answer(&mut event_loop, Duration::from_millis(300));
        assert_eq!(
            got,
            [
                "preview Some((0, 40, 400, 760))",
                "place 7 (0, 40, 400, 760)",
                "place 8 (0, 40, 400, 760)"
            ]
        );
        caller.join().unwrap();

        service.emit(Event::DragStarted(7));
        service.emit(Event::DragMotion(7, 0.0, 300.0));
        service.emit(Event::DragEnded(7, 0.0, 300.0));
        assert_eq!(events.next(), Some(client::Event::DragStarted(7)));
        assert_eq!(
            events.next(),
            Some(client::Event::DragMotion(7, 0.0, 300.0))
        );
        assert_eq!(events.next(), Some(client::Event::DragEnded(7, 0.0, 300.0)));

        // Leaving the bus hands tiling back to the compositor.
        drop(events);
        drop(tiling);
        drop(bus);
        assert_eq!(
            answer(&mut event_loop, Duration::from_millis(300)),
            ["tiler false"]
        );
    }

    #[test]
    fn only_the_last_of_a_run_of_motions_is_sent() {
        use Event::*;
        let events = vec![
            DragStarted(1),
            DragMotion(1, 0.0, 0.0),
            DragMotion(1, 5.0, 0.0),
            DragMotion(1, 9.0, 0.0),
            DragEnded(1, 9.0, 0.0),
            DragMotion(2, 1.0, 1.0),
        ];
        assert_eq!(
            coalesce(events),
            [
                DragStarted(1),
                DragMotion(1, 9.0, 0.0),
                DragEnded(1, 9.0, 0.0),
                DragMotion(2, 1.0, 1.0)
            ]
        );
    }
}
