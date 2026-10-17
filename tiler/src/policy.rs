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
}
