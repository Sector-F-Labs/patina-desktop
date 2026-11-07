//! Where a dragged monitor lands (ADR-0121): the side of another monitor it
//! goes to, and how far along that edge. Pure arithmetic in the desktop's
//! points, so the Displays page's drag is tested without a window.

use patina_system::displays::Side;

/// A monitor's place in the desktop, in points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct R {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl R {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    fn overlaps(self, o: R) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    fn centre(self) -> (f64, f64) {
        (
            self.x as f64 + self.w as f64 / 2.0,
            self.y as f64 + self.h as f64 / 2.0,
        )
    }
}

/// Where a monitor went: next to `others[to]`, on `side`, `offset` points
/// along the edge from where their top (or left) edges line up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snap {
    pub to: usize,
    pub side: Side,
    pub offset: i32,
}

impl Snap {
    /// The rectangle `moved`, of that size, ends up in.
    pub fn place(self, moved: R, others: &[R]) -> R {
        let o = others[self.to];
        let (x, y) = match self.side {
            Side::Right => (o.x + o.w, o.y + self.offset),
            Side::Left => (o.x - moved.w, o.y + self.offset),
            Side::Below => (o.x + self.offset, o.y + o.h),
            Side::Above => (o.x + self.offset, o.y - moved.h),
        };
        R::new(x, y, moved.w, moved.h)
    }
}

/// Along an edge, `moved_len` against `other_len`: the offset as it is, or
/// snapped to the edges lining up at the start, the middle or the end when
/// it's within a few points of one.
fn snap_along(offset: i32, moved_len: i32, other_len: i32) -> i32 {
    let near = (other_len / 20).max(16);
    let targets = [0, (other_len - moved_len) / 2, other_len - moved_len];
    targets
        .into_iter()
        .filter(|t| (offset - t).abs() <= near)
        .min_by_key(|t| (offset - t).abs())
        .unwrap_or(offset)
}

/// The nearest place beside one of `others` for `moved`, dragged to where it
/// is: never overlapping another, and touching along some of the edge.
pub fn snap(moved: R, others: &[R]) -> Option<Snap> {
    let (mx, my) = moved.centre();
    let mut best: Option<(f64, Snap)> = None;
    for (to, o) in others.iter().enumerate() {
        for side in [Side::Right, Side::Left, Side::Below, Side::Above] {
            // Along the edge: down it for left and right, across for the others.
            let (along, moved_len, other_len) = match side {
                Side::Right | Side::Left => (moved.y - o.y, moved.h, o.h),
                Side::Below | Side::Above => (moved.x - o.x, moved.w, o.w),
            };
            // Touching along at least a quarter of the shorter edge.
            let least = moved_len.min(other_len) / 4;
            let offset = snap_along(
                along.clamp(least - moved_len, other_len - least),
                moved_len,
                other_len,
            );
            let candidate = Snap { to, side, offset };
            let at = candidate.place(moved, others);
            if others.iter().any(|other| at.overlaps(*other)) {
                continue;
            }
            let (cx, cy) = at.centre();
            let cost = (cx - mx).powi(2) + (cy - my).powi(2);
            if best.is_none_or(|(b, _)| cost < b) {
                best = Some((cost, candidate));
            }
        }
    }
    best.map(|(_, snap)| snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAPTOP: R = R {
        x: 0,
        y: 0,
        w: 1600,
        h: 1000,
    };

    #[test]
    fn dragged_beside_it_goes_to_that_side() {
        let moved = R::new(1700, 20, 3440, 1440);
        let s = snap(moved, &[LAPTOP]).unwrap();
        assert_eq!((s.to, s.side), (0, Side::Right));
        // The tops line up: close enough to snap.
        assert_eq!(s.offset, 0);
        let above = R::new(30, -1500, 3440, 1440);
        assert_eq!(snap(above, &[LAPTOP]).unwrap().side, Side::Above);
        let left = R::new(-3500, 0, 3440, 1440);
        assert_eq!(snap(left, &[LAPTOP]).unwrap().side, Side::Left);
        let below = R::new(0, 1100, 1920, 1080);
        assert_eq!(snap(below, &[LAPTOP]).unwrap().side, Side::Below);
    }

    #[test]
    fn it_snaps_to_the_middle_and_the_bottom_but_not_between() {
        // 1080 against 1000: bottoms lining up is an offset of -80.
        let s = snap(R::new(1650, -75, 1920, 1080), &[LAPTOP]).unwrap();
        assert_eq!((s.side, s.offset), (Side::Right, -80));
        // Centres: -40.
        let s = snap(R::new(1650, -35, 1920, 1080), &[LAPTOP]).unwrap();
        assert_eq!(s.offset, -40);
        // Well between: left as dragged.
        let s = snap(R::new(1650, 300, 1920, 1080), &[LAPTOP]).unwrap();
        assert_eq!(s.offset, 300);
    }

    #[test]
    fn it_always_touches_along_the_edge() {
        // Dragged far below the right edge, it still touches the laptop.
        let s = snap(R::new(1650, 5000, 1920, 1080), &[LAPTOP]).unwrap();
        assert_eq!(s.side, Side::Below);
        let at = s.place(R::new(1650, 5000, 1920, 1080), &[LAPTOP]);
        assert!(at.x < LAPTOP.x + LAPTOP.w && at.y == 1000);
    }

    #[test]
    fn it_never_lands_on_another_monitor() {
        let right = R::new(1600, 0, 1920, 1080);
        // Dropped on the right of the laptop, where `right` already is:
        // goes elsewhere.
        let s = snap(R::new(1700, 100, 1280, 800), &[LAPTOP, right]).unwrap();
        let at = s.place(R::new(1700, 100, 1280, 800), &[LAPTOP, right]);
        assert!(!at.overlaps(LAPTOP) && !at.overlaps(right));
    }

    #[test]
    fn a_place_is_where_the_rule_in_the_compositor_puts_it() {
        let s = Snap {
            to: 0,
            side: Side::Above,
            offset: 40,
        };
        let at = s.place(R::new(0, 0, 3440, 1440), &[LAPTOP]);
        assert_eq!((at.x, at.y), (40, -1440));
    }
}
