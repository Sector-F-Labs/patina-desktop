//! patina-tiler: the Patina desktop's tiler (ADR-0053). The compositor
//! starts it and keeps it running, as it does the bar, and only reports
//! drags and Super+arrows and places windows where it's told
//! (`com.sectorflabs.Patina.Tiling`, ADR-0049); this decides, as Ubuntu
//! does. A window dragged to the left or right edge takes that half, and
//! to the top fills the screen, with a preview while it's held there.
//! Super+Left/Right tile to the halves, Up fills and Down restores.
//! A window opens where its app's window was last, if none of its app's
//! are open, and fills if it's too big for the screen (ADR-0065).
//!
//! It holds the Tiler name so another tiler can take over (as
//! `cargo run -p patina-system --example thirds` does), and decides again
//! when that one leaves. `policy.rs` is what tiles where; `positions.rs`
//! where apps' windows were.

mod policy;
mod positions;

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use patina_system::tiling::{self, Event, Rect, Tiling};
use policy::{Opening, Remembered, Snap, Tile};
use positions::Positions;

const USAGE: &str = "usage: patina-tiler

Decides how windows tile on the Patina desktop, over the compositor's
D-Bus tiling. patina-compositor starts it; see patina-tiler(1).";

/// How long to wait for the compositor's tiling on the bus, when both
/// start together.
const WAIT_FOR_COMPOSITOR: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-tiler {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("patina-tiler: {e}");
            ExitCode::FAILURE
        }
    }
}

/// What the tiler knows of the screen.
struct Tiler<'a> {
    bus: &'a zbus::blocking::Connection,
    tiling: Tiling<'a>,
    /// The tiling area, inside the outer gaps.
    area: Rect,
    gap: i32,
    screen: Rect,
    /// The windows this tiled, and how.
    tiled: HashMap<u64, Tile>,
    /// Where the columns meet, as dragged (ADR-0054); halves until then.
    split: f64,
    /// The window being dragged, if it can be tiled, and where a drop
    /// would put it.
    dragging: Option<(u64, Option<Tile>)>,
    /// Where each app's window was last.
    positions: Positions,
}

fn run() -> zbus::Result<()> {
    let bus = zbus::blocking::Connection::session()?;
    let tiling = Tiling::new(&bus)?;
    let started = Instant::now();
    let area = loop {
        match tiling.work_area() {
            Ok(area) => break area,
            Err(_) if started.elapsed() < WAIT_FOR_COMPOSITOR => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e),
        }
    };
    exit_with_compositor(&bus)?;
    let events = tiling.events()?;
    tiling::claim_default(&bus)?;
    let mut tiler = Tiler {
        bus: &bus,
        area,
        gap: tiling.gap()?,
        screen: tiling.screen()?,
        tiling,
        tiled: HashMap::new(),
        split: policy::HALVES,
        dragging: None,
        positions: Positions::load(),
    };
    for event in events {
        if let Err(e) = tiler.handle(event) {
            eprintln!("patina-tiler: {e}");
        }
    }
    Ok(())
}

/// Exits when the compositor this started with leaves the bus, or another
/// takes its name (a nested one from `make run`): otherwise this would
/// outlive it, still holding the Tiler name, and a newer tiler would queue
/// behind it for good. The compositor that wants a tiler starts its own.
fn exit_with_compositor(bus: &zbus::blocking::Connection) -> zbus::Result<()> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus)?;
    // Listening first, so a change between the two is heard.
    let changes = dbus.receive_name_owner_changed_with_args(&[(0, tiling::NAME)])?;
    let owner = dbus.get_name_owner(tiling::NAME.try_into()?)?;
    let owner = owner.as_str().to_string();
    std::thread::Builder::new()
        .name("compositor".into())
        .spawn(move || {
            for signal in changes {
                let Ok(args) = signal.args() else { continue };
                if args.name() == tiling::NAME
                    && args.new_owner().as_ref().map(|o| o.as_str()) != Some(owner.as_str())
                {
                    eprintln!("patina-tiler: the compositor left the bus; exiting");
                    std::process::exit(0);
                }
            }
        })
        .map_err(|e| zbus::Error::Failure(e.to_string()))?;
    Ok(())
}

impl Tiler<'_> {
    fn handle(&mut self, event: Event) -> zbus::Result<()> {
        match event {
            Event::DragStarted(id) => {
                // A dragged window is restored by the compositor.
                self.tiled.remove(&id);
                self.dragging = None;
                // Only windows the compositor lists can be tiled (not X11
                // ones yet).
                if tiling::deciding(self.bus) && self.listed(id)? {
                    self.dragging = Some((id, None));
                }
            }
            Event::DragMotion(id, x, y) => {
                let Some((dragged, zone)) = &mut self.dragging else {
                    return Ok(());
                };
                let now = policy::zone((x, y), self.screen);
                if *dragged != id || now == *zone {
                    return Ok(());
                }
                *zone = now;
                match now {
                    Some(tile) => self.tiling.show_preview(self.rect_for(id, tile)?)?,
                    None => self.tiling.hide_preview()?,
                }
            }
            Event::DragEnded(id, ..) => {
                if let Some((dragged, Some(tile))) = self.dragging.take()
                    && dragged == id
                {
                    self.place(id, tile)?;
                } else if let Some(window) = self.window(id)?
                    && window.placement.is_empty()
                {
                    // Dropped floating: kept now, in case the session
                    // ends before the window closes.
                    let at = (window.rect.0, window.rect.1);
                    let floated = Remembered { at, filled: false };
                    self.positions.set(&window.app_id, floated);
                }
            }
            Event::SnapRequested(id, direction) => {
                let Some(snap) = Snap::from_name(&direction) else {
                    return Ok(());
                };
                if !tiling::deciding(self.bus) {
                    return Ok(());
                }
                let placement = self.placement(id)?.unwrap_or_default();
                // Floated since it was tiled: resized by hand.
                if placement.is_empty() {
                    self.tiled.remove(&id);
                }
                let now = match self.tiled.get(&id) {
                    Some(tile) => Some(*tile),
                    // Maximised by the app, or filled by another tiler.
                    None => (!placement.is_empty()).then_some(Tile::Fill),
                };
                match policy::after_snap(now, snap) {
                    Some(tile) => self.place(id, tile)?,
                    None => {
                        self.tiled.remove(&id);
                        self.tiling.restore(id)?;
                    }
                }
            }
            // The line between tiled windows was dragged: the columns
            // keep where it was left.
            Event::Resized(id, rect) => {
                if let Some(tile) = self.tiled.get(&id)
                    && let Some(split) = policy::split_of(*tile, rect, self.area, self.gap)
                {
                    self.split = split;
                }
            }
            Event::WindowOpened {
                id,
                app_id,
                placement,
                rect,
            } => {
                // The app asked to be maximised, or another tiler decides.
                if !placement.is_empty() || !tiling::deciding(self.bus) {
                    return Ok(());
                }
                // A second window of an app cascades, rather than opening
                // on top of the first.
                let alone = !app_id.is_empty()
                    && !self
                        .tiling
                        .windows()?
                        .iter()
                        .any(|w| w.id != id && w.app_id == app_id);
                let remembered = alone.then(|| self.positions.get(&app_id)).flatten();
                match policy::opening(rect, remembered, self.area) {
                    Opening::Stay => {}
                    Opening::Move(x, y) => self.tiling.move_to(id, (x, y))?,
                    Opening::Fill => self.place(id, Tile::Fill)?,
                }
            }
            Event::WindowClosed { id, app_id, rect } => {
                let filled = self.tiled.remove(&id) == Some(Tile::Fill);
                let at = (rect.0, rect.1);
                self.positions.set(&app_id, Remembered { at, filled });
            }
            Event::WorkAreaChanged(area) => {
                self.area = area;
                self.gap = self.tiling.gap()?;
                self.screen = self.tiling.screen()?;
                self.relayout()?;
            }
        }
        Ok(())
    }

    /// Where `tile` puts window `id`: beside the columns as they are, if
    /// another window is in one; beside a half that was stretched sideways,
    /// taking what's left; otherwise halves again.
    fn rect_for(&self, id: u64, tile: Tile) -> zbus::Result<Rect> {
        let split = self.split_for(id, tile)?;
        Ok(policy::rect_for(tile, self.area, self.gap, split))
    }

    fn split_for(&self, id: u64, tile: Tile) -> zbus::Result<f64> {
        if self.beside(id) {
            return Ok(self.split);
        }
        let others: Vec<Rect> = self
            .tiling
            .windows()?
            .iter()
            .filter(|w| w.id != id && w.placement.is_empty())
            .map(|w| w.rect)
            .collect();
        Ok(policy::split_beside(tile, self.area, self.gap, &others).unwrap_or(policy::HALVES))
    }

    /// Whether another window is in one of the columns.
    fn beside(&self, id: u64) -> bool {
        self.tiled
            .iter()
            .any(|(other, t)| *other != id && *t != Tile::Fill)
    }

    fn place(&mut self, id: u64, tile: Tile) -> zbus::Result<()> {
        let split = self.split_for(id, tile)?;
        let rect = policy::rect_for(tile, self.area, self.gap, split);
        if !self.beside(id) {
            self.split = split;
        }
        self.tiled.insert(id, tile);
        self.tiling.place(id, rect)
    }

    fn listed(&self, id: u64) -> zbus::Result<bool> {
        Ok(self.window(id)?.is_some())
    }

    fn window(&self, id: u64) -> zbus::Result<Option<tiling::Window>> {
        Ok(self.tiling.windows()?.into_iter().find(|w| w.id == id))
    }

    /// Window `id`'s placement as the compositor names it, if it's open.
    fn placement(&self, id: u64) -> zbus::Result<Option<String>> {
        Ok(self
            .tiling
            .windows()?
            .into_iter()
            .find(|w| w.id == id)
            .map(|w| w.placement))
    }

    /// Keeps the halves this tiled filling their halves of a new work
    /// area. The compositor keeps filled windows filling; windows since
    /// closed, maximised or placed by another tiler are forgotten.
    fn relayout(&mut self) -> zbus::Result<()> {
        if !tiling::deciding(self.bus) {
            return Ok(());
        }
        let windows = self.tiling.windows()?;
        self.tiled.retain(|id, tile| {
            windows.iter().any(|w| {
                w.id == *id
                    && match tile {
                        Tile::Fill => w.placement == "fill",
                        Tile::Left | Tile::Right => w.placement == "rect",
                    }
            })
        });
        let halves: Vec<(u64, Tile)> = self
            .tiled
            .iter()
            .filter(|(_, tile)| **tile != Tile::Fill)
            .map(|(id, tile)| (*id, *tile))
            .collect();
        for (id, tile) in halves {
            self.place(id, tile)?;
        }
        Ok(())
    }
}
