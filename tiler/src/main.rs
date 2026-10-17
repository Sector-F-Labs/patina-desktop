//! patina-tiler: the Patina desktop's tiler (ADR-0053). The compositor
//! starts it and keeps it running, as it does the bar, and only reports
//! drags and Super+arrows and places windows where it's told
//! (`com.sectorflabs.Patina.Tiling`, ADR-0049); this decides, as Ubuntu
//! does. A window dragged to the left or right edge takes that half, and
//! to the top fills the screen, with a preview while it's held there.
//! Super+Left/Right tile to the halves, Up fills and Down restores.
//!
//! It holds the Tiler name so another tiler can take over (as
//! `cargo run -p patina-system --example thirds` does), and decides again
//! when that one leaves. `policy.rs` is what tiles where.

mod policy;

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use patina_system::tiling::{self, Event, Rect, Tiling};
use policy::{Snap, Tile};

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
    /// The window being dragged, if it can be tiled, and where a drop
    /// would put it.
    dragging: Option<(u64, Option<Tile>)>,
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
    let events = tiling.events()?;
    tiling::claim_default(&bus)?;
    let mut tiler = Tiler {
        bus: &bus,
        area,
        gap: tiling.gap()?,
        screen: tiling.screen()?,
        tiling,
        tiled: HashMap::new(),
        dragging: None,
    };
    for event in events {
        if let Err(e) = tiler.handle(event) {
            eprintln!("patina-tiler: {e}");
        }
    }
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
                    Some(tile) => self
                        .tiling
                        .show_preview(policy::rect_for(tile, self.area, self.gap))?,
                    None => self.tiling.hide_preview()?,
                }
            }
            Event::DragEnded(id, ..) => {
                if let Some((dragged, Some(tile))) = self.dragging.take()
                    && dragged == id
                {
                    self.place(id, tile)?;
                }
            }
            Event::SnapRequested(id, direction) => {
                let Some(snap) = Snap::from_name(&direction) else {
                    return Ok(());
                };
                if !tiling::deciding(self.bus) {
                    return Ok(());
                }
                let now = match self.tiled.get(&id) {
                    Some(tile) => Some(*tile),
                    // Maximised by the app, or filled by another tiler.
                    None => self
                        .placement(id)?
                        .filter(|p| !p.is_empty())
                        .map(|_| Tile::Fill),
                };
                match policy::after_snap(now, snap) {
                    Some(tile) => self.place(id, tile)?,
                    None => {
                        self.tiled.remove(&id);
                        self.tiling.restore(id)?;
                    }
                }
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

    fn place(&mut self, id: u64, tile: Tile) -> zbus::Result<()> {
        self.tiled.insert(id, tile);
        self.tiling
            .place(id, policy::rect_for(tile, self.area, self.gap))
    }

    fn listed(&self, id: u64) -> zbus::Result<bool> {
        Ok(self.tiling.windows()?.iter().any(|w| w.id == id))
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
