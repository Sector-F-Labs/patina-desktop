//! Where the monitors sit relative to each other (ADR-0121): the arithmetic
//! behind "extend the desktop". Pure, so it's tested without a GPU.
//!
//! Each monitor is told by an opaque `id` (the backend picks it: the EDID's
//! make, model and serial where there is one, else the connector's name) and
//! says where it goes as a [`Place`]: a side of another monitor, plus an
//! offset along that edge. The first monitor, the primary, is at the origin.
//! One without a place goes to the right of the one before it, which is where
//! a monitor never seen before lands until Settings moves it.

// Places and sides aren't saved or set yet (ADR-0121): drop this when they are.
#![allow(dead_code)]

use smithay::utils::{Logical, Point, Rectangle, Size};

/// What one monitor shows: its place in the desktop and its scale. Things
/// drawn in desktop coordinates are moved by `area.loc` to be drawn on it.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub area: Rectangle<i32, Logical>,
    pub scale: f64,
}

/// Which edge of the other monitor this one touches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
    LeftOf,
    RightOf,
}

/// Next to monitor `to`, on `side`, slid `offset` logical pixels along the
/// shared edge (down for left and right, right for above and below) from
/// where their top or left edges line up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub to: String,
    pub side: Side,
    pub offset: i32,
}

/// A monitor to place: its logical size (the mode over the scale) and where
/// it was asked to go.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub size: Size<i32, Logical>,
    pub place: Option<Place>,
}

/// The origin of each entry, in the order given. A place that names a
/// monitor not in the list, or that leads back to itself, is as no place.
pub fn arrange(entries: &[Entry]) -> Vec<(String, Point<i32, Logical>)> {
    let mut at: Vec<Option<Point<i32, Logical>>> = vec![None; entries.len()];
    for i in 0..entries.len() {
        resolve(entries, &mut at, i, &mut Vec::new());
    }
    entries
        .iter()
        .zip(at)
        .map(|(e, p)| (e.id.clone(), p.unwrap_or_default()))
        .collect()
}

fn resolve(
    entries: &[Entry],
    at: &mut Vec<Option<Point<i32, Logical>>>,
    i: usize,
    visiting: &mut Vec<usize>,
) -> Point<i32, Logical> {
    if let Some(p) = at[i] {
        return p;
    }
    visiting.push(i);
    let entry = &entries[i];
    // The primary is the origin, wherever it was asked to be.
    let anchor = entry
        .place
        .as_ref()
        .filter(|_| i != 0)
        .and_then(|p| Some((p, entries.iter().position(|e| e.id == p.to)?)))
        .filter(|(_, j)| !visiting.contains(j));
    let point = match anchor {
        Some((place, j)) => {
            let origin = resolve(entries, at, j, visiting);
            let other = entries[j].size;
            match place.side {
                Side::RightOf => (origin.x + other.w, origin.y + place.offset),
                Side::LeftOf => (origin.x - entry.size.w, origin.y + place.offset),
                Side::Below => (origin.x + place.offset, origin.y + other.h),
                Side::Above => (origin.x + place.offset, origin.y - entry.size.h),
            }
            .into()
        }
        // The primary, or one with nowhere to be: after the one before.
        None if i == 0 => Point::default(),
        None => {
            let before = resolve(entries, at, i - 1, visiting);
            (before.x + entries[i - 1].size.w, before.y).into()
        }
    };
    visiting.pop();
    at[i] = Some(point);
    point
}

/// Shifts every origin so the top-left of the whole desktop is (0, 0), as
/// Smithay's space and the clients' coordinates are happiest with: a monitor
/// placed above or left of the primary would otherwise be negative.
pub fn normalize(placed: &mut [(String, Point<i32, Logical>)]) {
    let left = placed.iter().map(|(_, p)| p.x).min().unwrap_or(0);
    let top = placed.iter().map(|(_, p)| p.y).min().unwrap_or(0);
    for (_, p) in placed {
        p.x -= left;
        p.y -= top;
    }
}

fn rect(origin: Point<i32, Logical>, size: Size<i32, Logical>) -> Rectangle<i32, Logical> {
    Rectangle::new(origin, size)
}

/// The monitor under `point`, as an index into `monitors`.
pub fn monitor_at(
    monitors: &[Rectangle<i32, Logical>],
    point: Point<f64, Logical>,
) -> Option<usize> {
    monitors.iter().position(|m| {
        point.x >= m.loc.x as f64
            && point.x < (m.loc.x + m.size.w) as f64
            && point.y >= m.loc.y as f64
            && point.y < (m.loc.y + m.size.h) as f64
    })
}

/// `point` if it's on a monitor, else the nearest point that is: the
/// pointer stops at the edge of the desktop, slides along it, and passes
/// from one monitor to the next wherever they touch (and stays put where
/// they don't, as on a real desk).
pub fn clamp(
    monitors: &[Rectangle<i32, Logical>],
    point: Point<f64, Logical>,
) -> Point<f64, Logical> {
    let mut best: Option<(f64, Point<f64, Logical>)> = None;
    for m in monitors {
        let x = point
            .x
            .clamp(m.loc.x as f64, (m.loc.x + m.size.w - 1) as f64);
        let y = point
            .y
            .clamp(m.loc.y as f64, (m.loc.y + m.size.h - 1) as f64);
        let d = (x - point.x).powi(2) + (y - point.y).powi(2);
        if best.is_none_or(|(b, _)| d < b) {
            best = Some((d, Point::from((x, y))));
        }
    }
    best.map_or(point, |(_, p)| p)
}

/// The rectangles of `entries` once placed.
pub fn rects(entries: &[Entry]) -> Vec<Rectangle<i32, Logical>> {
    let mut placed = arrange(entries);
    normalize(&mut placed);
    placed
        .iter()
        .zip(entries)
        .map(|((_, p), e)| rect(*p, e.size))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, w: i32, h: i32, place: Option<(&str, Side, i32)>) -> Entry {
        Entry {
            id: id.into(),
            size: (w, h).into(),
            place: place.map(|(to, side, offset)| Place {
                to: to.into(),
                side,
                offset,
            }),
        }
    }

    fn pos(placed: &[(String, Point<i32, Logical>)], id: &str) -> (i32, i32) {
        let p = placed.iter().find(|(i, _)| i == id).unwrap().1;
        (p.x, p.y)
    }

    #[test]
    fn a_new_monitor_goes_to_the_right() {
        let placed = arrange(&[
            entry("eDP-1", 1280, 800, None),
            entry("DP-1", 1920, 1080, None),
        ]);
        assert_eq!(pos(&placed, "eDP-1"), (0, 0));
        assert_eq!(pos(&placed, "DP-1"), (1280, 0));
    }

    #[test]
    fn above_is_negative_until_normalised() {
        let mut placed = arrange(&[
            entry("eDP-1", 1280, 800, None),
            entry("DP-1", 1920, 1080, Some(("eDP-1", Side::Above, 0))),
        ]);
        assert_eq!(pos(&placed, "DP-1"), (0, -1080));
        normalize(&mut placed);
        assert_eq!(pos(&placed, "DP-1"), (0, 0));
        assert_eq!(pos(&placed, "eDP-1"), (0, 1080));
    }

    #[test]
    fn left_below_and_offsets() {
        let placed = arrange(&[
            entry("a", 1000, 600, None),
            entry("b", 400, 300, Some(("a", Side::LeftOf, 50))),
            entry("c", 500, 200, Some(("a", Side::Below, 100))),
        ]);
        assert_eq!(pos(&placed, "b"), (-400, 50));
        assert_eq!(pos(&placed, "c"), (100, 600));
    }

    #[test]
    fn a_place_can_lead_through_another_monitor() {
        // Listed before the one it hangs off.
        let placed = arrange(&[
            entry("a", 100, 100, None),
            entry("c", 100, 100, Some(("b", Side::RightOf, 0))),
            entry("b", 100, 100, Some(("a", Side::RightOf, 0))),
        ]);
        assert_eq!(pos(&placed, "c"), (200, 0));
    }

    #[test]
    fn nonsense_places_fall_back() {
        // Unknown target and a cycle: neither hangs or panics.
        let placed = arrange(&[
            entry("a", 100, 100, Some(("b", Side::RightOf, 0))),
            entry("b", 100, 100, Some(("a", Side::RightOf, 0))),
            entry("c", 100, 100, Some(("gone", Side::Above, 0))),
        ]);
        assert_eq!(placed.len(), 3);
        assert_eq!(pos(&placed, "a"), (0, 0));
    }

    #[test]
    fn the_pointer_crosses_where_monitors_touch() {
        let monitors = rects(&[entry("a", 100, 100, None), entry("b", 100, 100, None)]);
        // Past a's right edge is b's: it stays where it is.
        let p = Point::from((150.0, 50.0));
        assert_eq!(clamp(&monitors, p), p);
        assert_eq!(monitor_at(&monitors, p), Some(1));
        // Off the end of the desktop it stops at the edge.
        assert_eq!(clamp(&monitors, (250.0, 50.0).into()), (199.0, 50.0).into());
    }

    #[test]
    fn the_pointer_is_held_where_monitors_dont_touch() {
        // b is smaller and sits at the top: below it, next to a, is nowhere.
        let monitors = rects(&[entry("a", 100, 100, None), entry("b", 100, 40, None)]);
        assert_eq!(monitor_at(&monitors, (150.0, 80.0).into()), None);
        assert_eq!(clamp(&monitors, (150.0, 80.0).into()), (150.0, 39.0).into());
    }
}
