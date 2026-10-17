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

/// The rectangle `tile` fills in `area` (the tiling area, inside the outer
/// gaps), with `gap` between the halves. The left half takes the odd pixel.
pub fn rect_for(tile: Tile, (x, y, w, h): Rect, gap: i32) -> Rect {
    let left = (w - gap + 1) / 2;
    match tile {
        Tile::Fill => (x, y, w, h),
        Tile::Left => (x, y, left, h),
        Tile::Right => (x + left + gap, y, w - left - gap, h),
    }
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
        assert_eq!(rect_for(Tile::Left, area, 0), (0, 0, 641, 759));
        assert_eq!(rect_for(Tile::Right, area, 0), (641, 0, 640, 759));
        assert_eq!(rect_for(Tile::Fill, area, 0), area);
    }

    #[test]
    fn gaps_go_between_the_halves() {
        let area = (8, 8, 1265, 743);
        let left = rect_for(Tile::Left, area, 8);
        let right = rect_for(Tile::Right, area, 8);
        assert_eq!(left, (8, 8, 629, 743));
        assert_eq!(right, (645, 8, 628, 743));
        assert_eq!(right.0 - (left.0 + left.2), 8);
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
