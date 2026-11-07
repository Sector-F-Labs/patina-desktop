//! What tiles where, Ubuntu's way: the decisions alone, with no bus, so
//! they're easy to change and test.

use patina_system::tiling::Rect;

/// How near the screen's edge, in logical pixels, the pointer must be for
/// a drop to tile.
pub const EDGE: f64 = 4.0;

/// Where the tiler puts a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Left,
    Right,
    /// All of the tiling area: filled, not maximised (ADR-0052).
    Fill,
}

/// Where a window dropped with the pointer at (`x`, `y`) on `screen`
/// tiles: the left or right half at those edges, filling at the top. The
/// sides win in the top corners.
pub fn zone((x, y): (f64, f64), screen: Rect) -> Option<Tile> {
    let (left, top) = (screen.0 as f64, screen.1 as f64);
    let right = left + screen.2 as f64;
    if x < left + EDGE {
        Some(Tile::Left)
    } else if x >= right - EDGE {
        Some(Tile::Right)
    } else if y < top + EDGE {
        Some(Tile::Fill)
    } else {
        None
    }
}

/// Where the left column ends, as a share of the room beside the gap:
/// halves, until the line between tiled windows is dragged (ADR-0054).
pub const HALVES: f64 = 0.5;

/// The rectangle `tile` fills in `area` (the tiling area, inside the outer
/// gaps), with `gap` between the columns and `split` of the room left of
/// it. Even halves give the left the odd pixel.
pub fn rect_for(tile: Tile, (x, y, w, h): Rect, gap: i32, split: f64) -> Rect {
    let room = (w - gap).max(0);
    let left = ((room as f64 * split).round() as i32).clamp(0, room);
    match tile {
        Tile::Fill => (x, y, w, h),
        Tile::Left => (x, y, left, h),
        Tile::Right => (x + left + gap, y, w - left - gap, h),
    }
}

/// The split a `tile` window at `rect` of `area` shows, after the line
/// beside it was dragged; `None` for a window that isn't in a column.
pub fn split_of(tile: Tile, (_, _, w, _): Rect, area: Rect, gap: i32) -> Option<f64> {
    let room = (area.2 - gap).max(1) as f64;
    match tile {
        Tile::Left => Some(w as f64 / room),
        Tile::Right => Some((room - w as f64) / room),
        Tile::Fill => None,
    }
    .map(|s| s.clamp(0.0, 1.0))
}

/// How many pixels off an edge still counts as on it.
const SLACK: i32 = 2;

/// The split a window tiled as `tile` should take, when `others` (the other
/// windows' rectangles) hold a window that is as tall as the tiling area,
/// against the opposite side and wider than a half: a half tile that was
/// stretched sideways. `tile` then takes what is left of the area beside
/// it, not a half over the top of it. `None` if no window does that.
pub fn split_beside(tile: Tile, area: Rect, gap: i32, others: &[Rect]) -> Option<f64> {
    let (ax, ay, aw, ah) = area;
    let room = (aw - gap).max(1) as f64;
    let near = |a: i32, b: i32| (a - b).abs() <= SLACK;
    let widest = others
        .iter()
        .filter(|&&(x, y, w, h)| {
            near(y, ay)
                && near(h, ah)
                && match tile {
                    Tile::Right => near(x, ax),
                    Tile::Left => near(x + w, ax + aw),
                    Tile::Fill => false,
                }
                && w as f64 > room / 2.0
                && (w as f64) < room
        })
        .map(|r| r.2)
        .max()?;
    let taken = widest as f64 / room;
    Some(match tile {
        Tile::Right => taken,
        _ => 1.0 - taken,
    })
}

/// A Super+arrow's direction, as the compositor names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Up,
    Down,
    Left,
    Right,
}

impl Snap {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "up" => Snap::Up,
            "down" => Snap::Down,
            "left" => Snap::Left,
            "right" => Snap::Right,
            _ => return None,
        })
    }
}

/// What a Super+arrow does to a window tiled as `now`, Ubuntu's way: up
/// fills; down restores; left and right tile to that half, or restore from
/// the other one.
pub fn after_snap(now: Option<Tile>, snap: Snap) -> Option<Tile> {
    use Tile::*;
    match (snap, now) {
        (Snap::Up, _) => Some(Fill),
        (Snap::Down, _) => None,
        (Snap::Left, Some(Right)) | (Snap::Right, Some(Left)) => None,
        (Snap::Left, _) => Some(Left),
        (Snap::Right, _) => Some(Right),
    }
}

/// Where an app's window was last (ADR-0065).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remembered {
    /// Its top left where it floated, or did before it was filled.
    pub at: (i32, i32),
    /// It closed filling the work area.
    pub filled: bool,
}

/// What happens to a window as it opens (ADR-0065).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Where the compositor put it.
    Stay,
    /// Its top left here: where its app's window was, or moved back
    /// inside the area.
    Move(i32, i32),
    /// Too big for the area: it fills it instead of hanging off the screen.
    Fill,
}

/// Where a window opening at `rect` goes in `area`: filling if its app's
/// window closed filled, or if it's too big to fit at all; otherwise where
/// its app's window was, if `remembered`, moved back inside if it sticks
/// out.
pub fn opening(rect: Rect, remembered: Option<Remembered>, area: Rect) -> Opening {
    let (x, y, w, h) = rect;
    let (ax, ay, aw, ah) = area;
    if w > aw || h > ah || remembered.is_some_and(|r| r.filled) {
        return Opening::Fill;
    }
    let (want_x, want_y) = remembered.map_or((x, y), |r| r.at);
    let at = (want_x.clamp(ax, ax + aw - w), want_y.clamp(ay, ay + ah - h));
    if at == (x, y) {
        Opening::Stay
    } else {
        Opening::Move(at.0, at.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = (0, 0, 1280, 800);

    #[test]
    fn the_edges_tile_as_on_ubuntu() {
        assert_eq!(zone((0.0, 400.0), SCREEN), Some(Tile::Left));
        assert_eq!(zone((1279.0, 400.0), SCREEN), Some(Tile::Right));
        assert_eq!(zone((640.0, 0.0), SCREEN), Some(Tile::Fill));
        assert_eq!(zone((640.0, 400.0), SCREEN), None);
        assert_eq!(zone((10.0, 400.0), SCREEN), None, "near isn't at");
    }

    #[test]
    fn the_sides_win_in_the_top_corners() {
        assert_eq!(zone((0.0, 0.0), SCREEN), Some(Tile::Left));
        assert_eq!(zone((1279.0, 0.0), SCREEN), Some(Tile::Right));
    }

    #[test]
    fn halves_split_the_area() {
        let area = (0, 0, 1281, 759);
        assert_eq!(rect_for(Tile::Left, area, 0, HALVES), (0, 0, 641, 759));
        assert_eq!(rect_for(Tile::Right, area, 0, HALVES), (641, 0, 640, 759));
        assert_eq!(rect_for(Tile::Fill, area, 0, HALVES), area);
    }

    #[test]
    fn gaps_go_between_the_halves() {
        let area = (8, 8, 1265, 743);
        let left = rect_for(Tile::Left, area, 8, HALVES);
        let right = rect_for(Tile::Right, area, 8, HALVES);
        assert_eq!(left, (8, 8, 629, 743));
        assert_eq!(right, (645, 8, 628, 743));
        assert_eq!(right.0 - (left.0 + left.2), 8);
    }

    #[test]
    fn a_dragged_split_is_kept_and_the_other_side_takes_the_rest() {
        let area = (8, 8, 1265, 743);
        // The line dragged so the left window is 800 wide.
        let left = (8, 8, 800, 743);
        let split = split_of(Tile::Left, left, area, 8).unwrap();
        assert_eq!(rect_for(Tile::Left, area, 8, split), left);
        let right = rect_for(Tile::Right, area, 8, split);
        assert_eq!(right, (816, 8, 457, 743), "the rest, past the gap");
        assert_eq!(split_of(Tile::Right, right, area, 8), Some(split));
        assert_eq!(split_of(Tile::Fill, area, area, 8), None);
    }

    #[test]
    fn a_stretched_half_leaves_only_the_rest() {
        let area = (8, 8, 1265, 743);
        // A window as tall as a half, pulled out to 800 wide on the left.
        let wide = (8, 8, 800, 743);
        let split = split_beside(Tile::Right, area, 8, &[wide]).unwrap();
        assert_eq!(rect_for(Tile::Right, area, 8, split), (816, 8, 457, 743));
        // The same, from the right: the new left tile takes the rest.
        let wide = (473, 8, 800, 743);
        let split = split_beside(Tile::Left, area, 8, &[wide]).unwrap();
        assert_eq!(rect_for(Tile::Left, area, 8, split), (8, 8, 457, 743));
    }

    #[test]
    fn only_a_stretched_half_counts() {
        let area = (8, 8, 1265, 743);
        let none = |tile, r| split_beside(tile, area, 8, &[r]);
        // A half or less, shorter, on the same side, or all of the area.
        assert_eq!(none(Tile::Right, (8, 8, 600, 743)), None);
        assert_eq!(none(Tile::Right, (8, 8, 800, 500)), None);
        assert_eq!(none(Tile::Right, (8, 100, 800, 643)), None);
        assert_eq!(none(Tile::Left, (8, 8, 800, 743)), None);
        assert_eq!(none(Tile::Right, area), None);
        assert_eq!(none(Tile::Fill, (8, 8, 800, 743)), None);
    }

    #[test]
    fn snapping_follows_ubuntu() {
        use Tile::*;
        assert_eq!(after_snap(None, Snap::Up), Some(Fill));
        assert_eq!(after_snap(Some(Left), Snap::Up), Some(Fill));
        assert_eq!(after_snap(Some(Fill), Snap::Down), None);
        assert_eq!(after_snap(None, Snap::Left), Some(Left));
        assert_eq!(after_snap(Some(Fill), Snap::Left), Some(Left));
        assert_eq!(
            after_snap(Some(Right), Snap::Left),
            None,
            "back from the other half"
        );
        assert_eq!(after_snap(Some(Left), Snap::Right), None);
        assert_eq!(after_snap(Some(Left), Snap::Left), Some(Left));
    }

    const AREA: Rect = (8, 48, 1264, 744);

    fn floated(x: i32, y: i32) -> Option<Remembered> {
        Some(Remembered {
            at: (x, y),
            filled: false,
        })
    }

    #[test]
    fn a_window_opens_where_its_app_was() {
        let rect = (48, 88, 600, 400);
        assert_eq!(
            opening(rect, floated(300, 200), AREA),
            Opening::Move(300, 200)
        );
        assert_eq!(opening(rect, None, AREA), Opening::Stay);
        assert_eq!(opening(rect, floated(48, 88), AREA), Opening::Stay);
    }

    #[test]
    fn a_window_sticking_out_moves_back_inside() {
        // Remembered from a bigger screen: off the right and the bottom.
        assert_eq!(
            opening((48, 88, 600, 400), floated(1500, 900), AREA),
            Opening::Move(672, 392)
        );
        // Under the bar.
        assert_eq!(
            opening((48, 88, 600, 400), floated(100, 0), AREA),
            Opening::Move(100, 48)
        );
        // Cascaded, but too tall to fit there.
        assert_eq!(
            opening((48, 88, 600, 744), None, AREA),
            Opening::Move(48, 48)
        );
    }

    #[test]
    fn a_window_too_big_fills() {
        assert_eq!(opening((48, 88, 1265, 400), None, AREA), Opening::Fill);
        assert_eq!(
            opening((48, 88, 600, 745), floated(8, 48), AREA),
            Opening::Fill
        );
    }

    #[test]
    fn a_window_that_closed_filled_opens_filled() {
        let filled = Some(Remembered {
            at: (100, 100),
            filled: true,
        });
        assert_eq!(opening((48, 88, 600, 400), filled, AREA), Opening::Fill);
    }
}
