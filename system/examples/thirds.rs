//! A tiler of its own, to show the compositor's tiling can be decided by
//! any app (ADR-0049): windows tile in thirds rather than halves. Dropped at
//! the left or right edge, a window takes that third; at the top, the middle
//! one. Super+Left and Right step a window along the thirds, Up fills
//! the screen and Down restores. While it runs, the compositor's own tiling is off;
//! Ctrl+C gives it back.
//!
//! `cargo run -p patina-system --example thirds`, inside the desktop.

use std::collections::HashMap;

use patina_system::tiling::{Event, Rect, Tiling, claim};

/// How near the screen's edge the pointer is to tile.
const EDGE: f64 = 4.0;

/// The `n`th of three columns of `area`, with `gap` between them.
fn third((x, y, w, h): Rect, gap: i32, n: i32) -> Rect {
    let column = (w - 2 * gap) / 3;
    let left = x + n * (column + gap);
    let right = if n == 2 { x + w } else { left + column };
    (left, y, right - left, h)
}

/// Which third a drop at (`px`, `py`) takes: at the sides, those; at the
/// top, the middle.
fn column(screen_width: i32, px: f64, py: f64) -> Option<i32> {
    if px < EDGE {
        Some(0)
    } else if px >= screen_width as f64 - EDGE {
        Some(2)
    } else if py < EDGE {
        Some(1)
    } else {
        None
    }
}

fn main() -> zbus::Result<()> {
    let bus = zbus::blocking::Connection::session()?;
    let tiling = Tiling::new(&bus)?;
    let events = tiling.events()?;
    claim(&bus)?;
    let mut area = tiling.work_area()?;
    let mut gap = tiling.gap()?;
    // The screen is as wide as the work area and the gaps beside it; the
    // bar only takes height.
    let screen_width = area.0 + area.2 + gap;
    println!("tiling in thirds of {area:?}");
    // The column a dragged window would drop into, and each window's.
    let mut dropping: Option<i32> = None;
    let mut columns: HashMap<u64, i32> = HashMap::new();
    for event in events {
        let result = match event {
            Event::DragStarted(id) => {
                dropping = None;
                columns.remove(&id);
                Ok(())
            }
            Event::DragMotion(_, x, y) => {
                let now = column(screen_width, x, y);
                if now == dropping {
                    continue;
                }
                dropping = now;
                match now {
                    Some(n) => tiling.show_preview(third(area, gap, n)),
                    None => tiling.hide_preview(),
                }
            }
            Event::DragEnded(id, ..) => match dropping.take() {
                Some(n) => {
                    columns.insert(id, n);
                    tiling.place(id, third(area, gap, n))
                }
                None => Ok(()),
            },
            Event::SnapRequested(id, direction) => {
                let at = columns.get(&id).copied();
                let next = match (direction.as_str(), at) {
                    ("left", Some(n)) => Some((n - 1).max(0)),
                    ("left", None) => Some(0),
                    ("right", Some(n)) => Some((n + 1).min(2)),
                    ("right", None) => Some(2),
                    _ => None,
                };
                match (direction.as_str(), next) {
                    (_, Some(n)) => {
                        columns.insert(id, n);
                        tiling.place(id, third(area, gap, n))
                    }
                    ("up", _) => {
                        columns.remove(&id);
                        tiling.place(id, area)
                    }
                    _ => {
                        columns.remove(&id);
                        tiling.restore(id)
                    }
                }
            }
            Event::WorkAreaChanged(new) => {
                area = new;
                tiling.gap().map(|g| gap = g)
            }
            // Thirds stay thirds: a dragged line lasts until the next tile.
            Event::Resized(..) => Ok(()),
        };
        if let Err(e) = result {
            eprintln!("thirds: {e}");
        }
    }
    Ok(())
}
