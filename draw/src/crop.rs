//! The part of the picture Crop keeps: a rectangle in picture pixels, and the handles that resize
//! it. No drawing here, only where things are; tested.

use egui::{CursorIcon, Pos2, Rect};

/// A rectangle of pixels, both corners included, `x0 <= x1` and `y0 <= y1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Region {
    /// The rectangle with these opposite corners, whichever way round they come.
    pub fn between(a: (i32, i32), b: (i32, i32)) -> Self {
        Self { x0: a.0.min(b.0), y0: a.1.min(b.1), x1: a.0.max(b.0), y1: a.1.max(b.1) }
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0 + 1
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0 + 1
    }

    pub fn contains(&self, p: (i32, i32)) -> bool {
        (self.x0..=self.x1).contains(&p.0) && (self.y0..=self.y1).contains(&p.1)
    }

    /// Where it is on screen: `origin` is the picture's top left and `zoom` the screen pixels to a
    /// picture pixel. The rectangle takes in the whole of its last pixel.
    pub fn on_screen(&self, origin: Pos2, zoom: f32) -> Rect {
        let at = |x: i32, y: i32| origin + egui::vec2(x as f32, y as f32) * zoom;
        Rect::from_min_max(at(self.x0, self.y0), at(self.x1 + 1, self.y1 + 1))
    }
}

/// One of the eight places to take hold of the rectangle: its corners and the middles of its sides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Left,
        Handle::Right,
        Handle::BottomLeft,
        Handle::Bottom,
        Handle::BottomRight,
    ];

    /// Which edges it moves: -1 the left or top one, 1 the right or bottom one, 0 neither.
    fn sides(self) -> (i32, i32) {
        match self {
            Handle::TopLeft => (-1, -1),
            Handle::Top => (0, -1),
            Handle::TopRight => (1, -1),
            Handle::Left => (-1, 0),
            Handle::Right => (1, 0),
            Handle::BottomLeft => (-1, 1),
            Handle::Bottom => (0, 1),
            Handle::BottomRight => (1, 1),
        }
    }

    /// Where it sits on `rect`, the rectangle on screen.
    pub fn position(self, rect: Rect) -> Pos2 {
        let (sx, sy) = self.sides();
        Pos2::new(
            [rect.left(), rect.center().x, rect.right()][(sx + 1) as usize],
            [rect.top(), rect.center().y, rect.bottom()][(sy + 1) as usize],
        )
    }

    /// The pointer that says it can be dragged: a double arrow across the edge or corner.
    pub fn cursor(self) -> CursorIcon {
        match self {
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
        }
    }
}

/// The handle of `rect` (on screen) within `radius` of `p`, the nearest if two are.
pub fn hit(rect: Rect, p: Pos2, radius: f32) -> Option<Handle> {
    Handle::ALL
        .into_iter()
        .map(|h| (h, h.position(rect).distance(p)))
        .filter(|(_, d)| *d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(h, _)| h)
}

/// `region` with the edges `handle` moves taken to `to`, kept inside a picture `size` pixels
/// across. Past the opposite edge it turns over, so the rectangle is always a rectangle.
pub fn resize(region: Region, handle: Handle, to: (i32, i32), size: (i32, i32)) -> Region {
    let to = (to.0.clamp(0, size.0 - 1), to.1.clamp(0, size.1 - 1));
    let (sx, sy) = handle.sides();
    let (mut a, mut b) = ((region.x0, region.y0), (region.x1, region.y1));
    match sx {
        -1 => a.0 = to.0,
        1 => b.0 = to.0,
        _ => {}
    }
    match sy {
        -1 => a.1 = to.1,
        1 => b.1 = to.1,
        _ => {}
    }
    Region::between(a, b)
}

/// `region` carried so the pointer at `to` is as far into it as `grab` (where it was taken hold
/// of, from its top left), kept inside the picture and the same size.
pub fn moved(region: Region, grab: (i32, i32), to: (i32, i32), size: (i32, i32)) -> Region {
    let (w, h) = (region.width(), region.height());
    let x0 = (to.0 - grab.0).clamp(0, (size.0 - w).max(0));
    let y0 = (to.1 - grab.1).clamp(0, (size.1 - h).max(0));
    Region { x0, y0, x1: x0 + w - 1, y1: y0 + h - 1 }
}

/// The shape the selection is held to.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Aspect {
    /// Any shape.
    Free,
    /// The picture's own.
    Original,
    /// Width to height, as 4:3 is.
    Fixed(u32, u32),
    /// Width to height, chosen by the user.
    Custom(u32, u32),
}

impl Aspect {
    /// Width over height for a picture `size` pixels across, or `None` if the shape is free.
    /// `portrait` turns a ratio on its side (4:3 becomes 3:4); the picture's own is never turned.
    pub fn ratio(self, size: (i32, i32), portrait: bool) -> Option<f32> {
        let given = |w: u32, h: u32| {
            let (w, h) = (w.max(1) as f32, h.max(1) as f32);
            Some(if portrait { h / w } else { w / h })
        };
        match self {
            Aspect::Free => None,
            Aspect::Original => Some(size.0.max(1) as f32 / size.1.max(1) as f32),
            Aspect::Fixed(w, h) | Aspect::Custom(w, h) => given(w, h),
        }
    }
}

/// The largest rectangle of shape `ratio` (width over height) that fits in `region`, in the
/// middle of it.
pub fn fit_ratio(region: Region, ratio: f32) -> Region {
    let (w, h) = (region.width() as f32, region.height() as f32);
    let width = w.min(h * ratio).max(1.0);
    let height = (width / ratio).min(h).max(1.0);
    let (width, height) = (width.round() as i32, height.round().max(1.0) as i32);
    let x0 = region.x0 + (region.width() - width) / 2;
    let y0 = region.y0 + (region.height() - height) / 2;
    Region { x0, y0, x1: x0 + width - 1, y1: y0 + height - 1 }
}

/// What to select when a shape is chosen and nothing is: the shape, in the middle of the picture
/// with a tenth left round it, so its handles show.
pub fn starting(ratio: f32, size: (i32, i32)) -> Region {
    let (mx, my) = (size.0 / 10, size.1 / 10);
    fit_ratio(Region { x0: mx, y0: my, x1: size.0 - 1 - mx, y1: size.1 - 1 - my }, ratio)
}

/// A rectangle of shape `ratio` with one corner at `anchor` and the opposite one as near `to` as
/// the shape and the picture (`size` pixels across) allow: it takes in the pointer where it can,
/// and shrinks, keeping its shape, where it would leave the picture.
pub fn from_corner(anchor: (i32, i32), to: (i32, i32), ratio: f32, size: (i32, i32)) -> Region {
    let sx = if to.0 >= anchor.0 { 1 } else { -1 };
    let sy = if to.1 >= anchor.1 { 1 } else { -1 };
    let (dx, dy) = ((to.0 - anchor.0).abs() + 1, (to.1 - anchor.1).abs() + 1);
    // Wide enough to take in the pointer both ways.
    let mut w = (dx as f32).max(dy as f32 * ratio);
    // Room to grow in the direction it's going.
    let room_x = if sx > 0 { size.0 - anchor.0 } else { anchor.0 + 1 } as f32;
    let room_y = if sy > 0 { size.1 - anchor.1 } else { anchor.1 + 1 } as f32;
    w = w.min(room_x).min(room_y * ratio).max(1.0);
    let width = w.round().max(1.0) as i32;
    let height = ((w / ratio).round().max(1.0) as i32).min(room_y as i32).max(1);
    Region::between(anchor, (anchor.0 + sx * (width - 1), anchor.1 + sy * (height - 1)))
}

/// A side handle's drag with a shape held: that side goes to `to` and the other way follows,
/// about the middle of the rectangle, until it meets the picture's edge, where the first gives way.
pub fn from_side(region: Region, handle: Handle, to: (i32, i32), ratio: f32, size: (i32, i32)) -> Region {
    let (sx, sy) = handle.sides();
    let to = (to.0.clamp(0, size.0 - 1), to.1.clamp(0, size.1 - 1));
    if sx != 0 {
        let fixed = if sx > 0 { region.x0 } else { region.x1 };
        let dir = if to.0 >= fixed { 1 } else { -1 };
        let mut w = (to.0 - fixed).abs() + 1;
        let cy = (region.y0 + region.y1) / 2;
        let most = 2 * cy.min(size.1 - 1 - cy) + 1;
        let mut h = ((w as f32 / ratio).round() as i32).max(1);
        if h > most {
            h = most;
            w = ((h as f32 * ratio).round() as i32).max(1);
        }
        let y0 = cy - (h - 1) / 2;
        Region::between((fixed, y0), (fixed + dir * (w - 1), y0 + h - 1))
    } else {
        let fixed = if sy > 0 { region.y0 } else { region.y1 };
        let dir = if to.1 >= fixed { 1 } else { -1 };
        let mut h = (to.1 - fixed).abs() + 1;
        let cx = (region.x0 + region.x1) / 2;
        let most = 2 * cx.min(size.0 - 1 - cx) + 1;
        let mut w = ((h as f32 * ratio).round() as i32).max(1);
        if w > most {
            w = most;
            h = ((w as f32 / ratio).round() as i32).max(1);
        }
        let x0 = cx - (w - 1) / 2;
        Region::between((x0, fixed), (x0 + w - 1, fixed + dir * (h - 1)))
    }
}

/// The corner of `region` opposite `handle`'s corner, which stays where it is when a corner is
/// dragged. `None` for a side.
pub fn opposite_corner(region: Region, handle: Handle) -> Option<(i32, i32)> {
    let (sx, sy) = handle.sides();
    (sx != 0 && sy != 0).then_some((if sx < 0 { region.x1 } else { region.x0 }, if sy < 0 { region.y1 } else { region.y0 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (i32, i32) = (100, 80);

    fn region() -> Region {
        Region { x0: 20, y0: 10, x1: 59, y1: 49 }
    }

    #[test]
    fn a_region_is_made_from_any_two_corners() {
        assert_eq!(Region::between((9, 7), (2, 3)), Region { x0: 2, y0: 3, x1: 9, y1: 7 });
        let r = region();
        assert_eq!((r.width(), r.height()), (40, 40));
        assert!(r.contains((20, 10)) && r.contains((59, 49)) && !r.contains((60, 49)));
    }

    #[test]
    fn a_corner_moves_two_edges_and_a_side_one() {
        let r = region();
        assert_eq!(resize(r, Handle::TopLeft, (5, 4), SIZE), Region { x0: 5, y0: 4, x1: 59, y1: 49 });
        assert_eq!(resize(r, Handle::BottomRight, (90, 70), SIZE), Region { x0: 20, y0: 10, x1: 90, y1: 70 });
        assert_eq!(resize(r, Handle::Top, (99, 2), SIZE), Region { x0: 20, y0: 2, x1: 59, y1: 49 }, "a side keeps the other axis");
        assert_eq!(resize(r, Handle::Right, (80, 0), SIZE), Region { x0: 20, y0: 10, x1: 80, y1: 49 });
        assert_eq!(resize(r, Handle::Left, (30, 0), SIZE), Region { x0: 30, y0: 10, x1: 59, y1: 49 });
        assert_eq!(resize(r, Handle::Bottom, (0, 60), SIZE), Region { x0: 20, y0: 10, x1: 59, y1: 60 });
    }

    #[test]
    fn dragging_past_the_other_side_turns_it_over_and_stays_in_the_picture() {
        let r = region();
        assert_eq!(resize(r, Handle::Left, (70, 0), SIZE), Region { x0: 59, y0: 10, x1: 70, y1: 49 });
        assert_eq!(resize(r, Handle::BottomRight, (500, -40), SIZE), Region { x0: 20, y0: 0, x1: 99, y1: 10 });
        assert_eq!(resize(r, Handle::TopLeft, (-9, -9), SIZE), Region { x0: 0, y0: 0, x1: 59, y1: 49 });
    }

    #[test]
    fn moving_keeps_the_size_and_the_grip_and_stops_at_the_edges() {
        let r = region();
        // Taken hold of 5 across and 5 down, carried to (50, 40): its corner goes to (45, 35).
        assert_eq!(moved(r, (5, 5), (50, 40), SIZE), Region { x0: 45, y0: 35, x1: 84, y1: 74 });
        assert_eq!(moved(r, (5, 5), (-30, -30), SIZE), Region { x0: 0, y0: 0, x1: 39, y1: 39 });
        let far = moved(r, (5, 5), (400, 400), SIZE);
        assert_eq!(far, Region { x0: 60, y0: 40, x1: 99, y1: 79 });
        assert_eq!((far.width(), far.height()), (40, 40));
    }

    #[test]
    fn handles_sit_on_the_corners_and_the_middles_of_the_sides() {
        let rect = Rect::from_min_max(Pos2::new(100.0, 50.0), Pos2::new(300.0, 250.0));
        assert_eq!(Handle::TopLeft.position(rect), Pos2::new(100.0, 50.0));
        assert_eq!(Handle::Top.position(rect), Pos2::new(200.0, 50.0));
        assert_eq!(Handle::BottomRight.position(rect), Pos2::new(300.0, 250.0));
        assert_eq!(Handle::Left.position(rect), Pos2::new(100.0, 150.0));
        assert_eq!(hit(rect, Pos2::new(104.0, 53.0), 9.0), Some(Handle::TopLeft));
        assert_eq!(hit(rect, Pos2::new(200.0, 56.0), 9.0), Some(Handle::Top));
        assert_eq!(hit(rect, Pos2::new(200.0, 150.0), 9.0), None, "the middle is not a handle");
    }

    #[test]
    fn the_nearest_handle_wins_on_a_tiny_selection() {
        let rect = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(106.0, 106.0));
        assert_eq!(hit(rect, Pos2::new(99.0, 99.0), 9.0), Some(Handle::TopLeft));
        assert_eq!(hit(rect, Pos2::new(107.0, 107.0), 9.0), Some(Handle::BottomRight));
    }

    fn shape(r: Region) -> f32 {
        r.width() as f32 / r.height() as f32
    }

    fn inside(r: Region) -> bool {
        r.x0 >= 0 && r.y0 >= 0 && r.x1 < SIZE.0 && r.y1 < SIZE.1
    }

    #[test]
    fn ratios_are_width_over_height_and_portrait_turns_them() {
        assert_eq!(Aspect::Free.ratio(SIZE, false), None);
        assert_eq!(Aspect::Original.ratio((200, 100), false), Some(2.0));
        assert_eq!(Aspect::Original.ratio((200, 100), true), Some(2.0), "the picture's own is not turned");
        assert_eq!(Aspect::Fixed(4, 3).ratio(SIZE, false), Some(4.0 / 3.0));
        assert_eq!(Aspect::Fixed(4, 3).ratio(SIZE, true), Some(3.0 / 4.0));
        assert_eq!(Aspect::Custom(0, 0).ratio(SIZE, false), Some(1.0), "zeros don't divide");
    }

    #[test]
    fn the_largest_rectangle_of_a_shape_fits_in_the_middle_of_a_region() {
        let wide = Region { x0: 0, y0: 0, x1: 99, y1: 79 };
        let square = fit_ratio(wide, 1.0);
        assert_eq!((square.width(), square.height()), (80, 80));
        assert_eq!((square.x0, square.y0), (10, 0), "in the middle");
        let tall = fit_ratio(wide, 0.5);
        assert_eq!((tall.width(), tall.height()), (40, 80));
        let flat = fit_ratio(wide, 4.0);
        assert_eq!((flat.width(), flat.height()), (100, 25));
        assert!(inside(square) && inside(tall) && inside(flat));
    }

    #[test]
    fn with_nothing_chosen_a_shape_starts_in_the_middle_with_room_round_it() {
        let r = starting(16.0 / 9.0, SIZE);
        assert!(inside(r) && r.x0 >= 10 && r.y0 >= 8 && r.x1 <= 89 && r.y1 <= 71, "{r:?}");
        assert!((shape(r) - 16.0 / 9.0).abs() < 0.1, "{}", shape(r));
    }

    #[test]
    fn a_corner_drag_keeps_its_shape_and_takes_in_the_pointer() {
        let r = from_corner((10, 10), (50, 20), 1.0, SIZE);
        assert_eq!((r.width(), r.height()), (41, 41), "wide enough for the pointer");
        assert_eq!((r.x0, r.y0), (10, 10), "the anchor stays");
        let up_left = from_corner((60, 60), (30, 50), 2.0, SIZE);
        assert_eq!((up_left.x1, up_left.y1), (60, 60), "growing the other way, the anchor still stays");
        assert!((shape(up_left) - 2.0).abs() < 0.1, "{up_left:?}");
        let out = from_corner((80, 60), (400, 400), 1.5, SIZE);
        assert!(inside(out), "kept in the picture: {out:?}");
        assert!((shape(out) - 1.5).abs() < 0.1, "and in shape: {out:?}");
        let tiny = from_corner((5, 5), (5, 5), 1.0, SIZE);
        assert_eq!((tiny.width(), tiny.height()), (1, 1));
    }

    #[test]
    fn a_side_drag_with_a_shape_carries_the_other_way_about_the_middle() {
        let r = Region { x0: 30, y0: 30, x1: 49, y1: 49 };
        let wider = from_side(r, Handle::Right, (69, 0), 1.0, SIZE);
        assert_eq!((wider.x0, wider.x1), (30, 69));
        assert_eq!((wider.width(), wider.height()), (40, 40));
        assert_eq!((wider.y0 + wider.y1) / 2, 39, "about the middle");
        let taller = from_side(r, Handle::Top, (0, 10), 1.0, SIZE);
        assert_eq!((taller.y0, taller.y1), (10, 49));
        assert_eq!(taller.width(), taller.height());
        // At the picture's edge the other way stops, and this one gives way.
        let edge = Region { x0: 30, y0: 5, x1: 49, y1: 24 };
        let held = from_side(edge, Handle::Right, (99, 0), 1.0, SIZE);
        assert!(inside(held) && (shape(held) - 1.0).abs() < 0.1, "{held:?}");
    }

    #[test]
    fn a_dragged_corner_holds_the_one_opposite() {
        let r = region();
        assert_eq!(opposite_corner(r, Handle::TopLeft), Some((59, 49)));
        assert_eq!(opposite_corner(r, Handle::BottomRight), Some((20, 10)));
        assert_eq!(opposite_corner(r, Handle::TopRight), Some((20, 49)));
        assert_eq!(opposite_corner(r, Handle::Top), None);
    }

    #[test]
    fn a_region_on_screen_takes_in_its_last_pixel() {
        let r = Region { x0: 2, y0: 1, x1: 3, y1: 1 };
        let rect = r.on_screen(Pos2::new(10.0, 20.0), 4.0);
        assert_eq!((rect.left(), rect.top(), rect.right(), rect.bottom()), (18.0, 24.0, 26.0, 28.0));
    }
}
