//! The picture: a bitmap of RGBA pixels and the drawing operations on it.
//!
//! Plain Rust with no egui in the drawing, so every tool is tested without a
//! window. Coordinates are signed so a stroke can run off the edge; anything
//! outside the bitmap is clipped.

pub type Rgba = [u8; 4];

pub const WHITE: Rgba = [255, 255, 255, 255];

#[derive(Clone, PartialEq, Debug)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<Rgba>,
}

/// How a closed shape is painted: its outline in the foreground colour, its
/// inside in the background colour, or both.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Fill {
    #[default]
    Outline,
    Filled,
    Both,
}

impl Bitmap {
    pub fn new(width: usize, height: usize, fill: Rgba) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        Self { width, height, pixels: vec![fill; width * height] }
    }

    pub fn from_rgba(width: usize, height: usize, bytes: &[u8]) -> Self {
        let pixels = bytes.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        Self { width, height, pixels }
    }

    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        self.index(x, y).map(|i| self.pixels[i])
    }

    pub fn put(&mut self, x: i32, y: i32, colour: Rgba) {
        if let Some(i) = self.index(x, y) {
            self.pixels[i] = colour;
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }

    /// A round dab `size` pixels across centred on (x, y).
    pub fn dab(&mut self, x: i32, y: i32, size: u32, colour: Rgba) {
        if size <= 1 {
            self.put(x, y, colour);
            return;
        }
        // Pixel centres are at +0.5, so an even size is centred between pixels.
        let r = size as f32 / 2.0;
        let lo = -(size as i32) / 2;
        for dy in lo..lo + size as i32 {
            for dx in lo..lo + size as i32 {
                let (fx, fy) = (dx as f32 + 0.5, dy as f32 + 0.5);
                // Size 2 and 3 would lose their corners to a strict circle.
                if fx * fx + fy * fy <= r * r + 0.25 {
                    self.put(x + dx, y + dy, colour);
                }
            }
        }
    }

    /// A line of dabs from `a` to `b` (Bresenham), without gaps at any speed.
    pub fn line(&mut self, a: (i32, i32), b: (i32, i32), size: u32, colour: Rgba) {
        let (mut x, mut y) = a;
        let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
        let (sx, sy) = (if a.0 < b.0 { 1 } else { -1 }, if a.1 < b.1 { 1 } else { -1 });
        let mut err = dx + dy;
        loop {
            self.dab(x, y, size, colour);
            if (x, y) == b {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// A rectangle with corners `a` and `b`, both included.
    pub fn rectangle(&mut self, a: (i32, i32), b: (i32, i32), size: u32, fill: Fill, stroke: Rgba, inside: Rgba) {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        let t = size.max(1) as i32;
        for y in y0.max(0)..=y1.min(self.height as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.width as i32 - 1) {
                let edge = x - x0 < t || x1 - x < t || y - y0 < t || y1 - y < t;
                if let Some(c) = shape_colour(edge, fill, stroke, inside) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// The ellipse inscribed in the rectangle with corners `a` and `b`.
    pub fn ellipse(&mut self, a: (i32, i32), b: (i32, i32), size: u32, fill: Fill, stroke: Rgba, inside: Rgba) {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        let (cx, cy) = ((x0 + x1 + 1) as f32 / 2.0, (y0 + y1 + 1) as f32 / 2.0);
        let (rx, ry) = ((x1 - x0 + 1) as f32 / 2.0, (y1 - y0 + 1) as f32 / 2.0);
        let t = size.max(1) as f32;
        let within = |x: f32, y: f32, rx: f32, ry: f32| {
            rx > 0.0 && ry > 0.0 && ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0
        };
        for y in y0.max(0)..=y1.min(self.height as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.width as i32 - 1) {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                if !within(fx, fy, rx, ry) {
                    continue;
                }
                let edge = !within(fx, fy, rx - t, ry - t);
                if let Some(c) = shape_colour(edge, fill, stroke, inside) {
                    self.put(x, y, c);
                }
            }
        }
    }

    /// Paints the region of one colour around (x, y), 4-connected, like the
    /// paint bucket. Does nothing when it is already `colour`.
    pub fn flood(&mut self, x: i32, y: i32, colour: Rgba) {
        let Some(target) = self.get(x, y) else { return };
        if target == colour {
            return;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            // Run along the row, then queue the rows above and below.
            let mut left = x;
            while self.get(left - 1, y) == Some(target) {
                left -= 1;
            }
            let (mut above, mut below) = (false, false);
            let mut x = left;
            while self.get(x, y) == Some(target) {
                self.put(x, y, colour);
                for (row, seen) in [(y - 1, &mut above), (y + 1, &mut below)] {
                    let open = self.get(x, row) == Some(target);
                    if open && !*seen {
                        stack.push((x, row));
                    }
                    *seen = open;
                }
                x += 1;
            }
        }
    }

    pub fn clear(&mut self, colour: Rgba) {
        self.pixels.fill(colour);
    }

    pub fn invert(&mut self) {
        for p in &mut self.pixels {
            *p = [255 - p[0], 255 - p[1], 255 - p[2], p[3]];
        }
    }

    pub fn flip_horizontal(&mut self) {
        for row in self.pixels.chunks_mut(self.width) {
            row.reverse();
        }
    }

    pub fn flip_vertical(&mut self) {
        let w = self.width;
        for y in 0..self.height / 2 {
            for x in 0..w {
                self.pixels.swap(y * w + x, (self.height - 1 - y) * w + x);
            }
        }
    }

    /// Turns the picture a quarter turn clockwise.
    pub fn rotate_clockwise(&mut self) {
        let (w, h) = (self.width, self.height);
        let mut turned = Vec::with_capacity(w * h);
        for y in 0..w {
            for x in 0..h {
                turned.push(self.pixels[(h - 1 - x) * w + y]);
            }
        }
        self.pixels = turned;
        (self.width, self.height) = (h, w);
    }

    /// Changes the canvas size, keeping the top left and filling new room.
    pub fn resize_canvas(&mut self, width: usize, height: usize, fill: Rgba) {
        let mut next = Bitmap::new(width, height, fill);
        for y in 0..self.height.min(next.height) {
            for x in 0..self.width.min(next.width) {
                next.pixels[y * next.width + x] = self.pixels[y * self.width + x];
            }
        }
        *self = next;
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.pixels.iter().flatten().copied().collect()
    }
}

fn shape_colour(edge: bool, fill: Fill, stroke: Rgba, inside: Rgba) -> Option<Rgba> {
    match (edge, fill) {
        (true, Fill::Outline | Fill::Both) => Some(stroke),
        (true, Fill::Filled) => Some(inside),
        (false, Fill::Filled | Fill::Both) => Some(inside),
        (false, Fill::Outline) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Rgba = [0, 0, 0, 255];
    const RED: Rgba = [255, 0, 0, 255];

    fn blank(w: usize, h: usize) -> Bitmap {
        Bitmap::new(w, h, WHITE)
    }

    fn count(b: &Bitmap, c: Rgba) -> usize {
        b.pixels.iter().filter(|p| **p == c).count()
    }

    #[test]
    fn a_pencil_dab_is_one_pixel() {
        let mut b = blank(5, 5);
        b.dab(2, 2, 1, BLACK);
        assert_eq!(count(&b, BLACK), 1);
        assert_eq!(b.get(2, 2), Some(BLACK));
    }

    #[test]
    fn a_brush_dab_is_round_and_grows_with_size() {
        let mut small = blank(20, 20);
        let mut large = blank(20, 20);
        small.dab(10, 10, 3, BLACK);
        large.dab(10, 10, 9, BLACK);
        assert!(count(&small, BLACK) >= 5);
        assert!(count(&large, BLACK) > count(&small, BLACK) * 4);
        // Round: the bounding box's corner stays clear.
        assert_eq!(large.get(6, 6), Some(WHITE));
        assert_eq!(large.get(10, 10), Some(BLACK));
    }

    #[test]
    fn drawing_off_the_edge_is_clipped() {
        let mut b = blank(4, 4);
        b.line((-10, -10), (20, 20), 3, BLACK);
        b.dab(-5, 100, 9, BLACK);
        assert_eq!(b.get(0, 0), Some(BLACK));
        assert_eq!(b.get(-1, 0), None);
    }

    #[test]
    fn a_line_has_no_gaps_in_any_direction() {
        for end in [(9, 0), (0, 9), (9, 9), (9, 3), (-9, 4), (2, -9)] {
            let mut b = blank(30, 30);
            let start = (15, 15);
            let end = (start.0 + end.0, start.1 + end.1);
            b.line(start, end, 1, BLACK);
            assert_eq!(b.get(start.0, start.1), Some(BLACK));
            assert_eq!(b.get(end.0, end.1), Some(BLACK));
            let steps = (end.0 - start.0).abs().max((end.1 - start.1).abs()) as usize + 1;
            assert_eq!(count(&b, BLACK), steps, "towards {end:?}");
        }
    }

    #[test]
    fn a_rectangle_outline_leaves_the_inside() {
        let mut b = blank(10, 10);
        b.rectangle((1, 1), (8, 8), 1, Fill::Outline, BLACK, RED);
        assert_eq!(b.get(1, 1), Some(BLACK));
        assert_eq!(b.get(8, 4), Some(BLACK));
        assert_eq!(b.get(4, 4), Some(WHITE));
        assert_eq!(count(&b, BLACK), 28);
    }

    #[test]
    fn rectangle_fills() {
        let mut both = blank(10, 10);
        both.rectangle((8, 8), (1, 1), 2, Fill::Both, BLACK, RED);
        assert_eq!(both.get(1, 1), Some(BLACK));
        assert_eq!(both.get(2, 2), Some(BLACK));
        assert_eq!(both.get(4, 4), Some(RED));
        let mut filled = blank(10, 10);
        filled.rectangle((1, 1), (8, 8), 1, Fill::Filled, BLACK, RED);
        assert_eq!(count(&filled, RED), 64);
        assert_eq!(count(&filled, BLACK), 0);
    }

    #[test]
    fn an_ellipse_stays_inside_its_box_and_is_hollow() {
        let mut b = blank(30, 30);
        b.ellipse((4, 6), (25, 21), 1, Fill::Outline, BLACK, RED);
        assert_eq!(b.get(15, 13), Some(WHITE));
        assert_eq!(b.get(4, 6), Some(WHITE));
        assert!(count(&b, BLACK) > 30);
        for y in 0..30 {
            for x in 0..30 {
                if b.get(x, y) == Some(BLACK) {
                    assert!((4..=25).contains(&x) && (6..=21).contains(&y));
                }
            }
        }
        let mut f = blank(30, 30);
        f.ellipse((4, 6), (25, 21), 3, Fill::Both, BLACK, RED);
        assert_eq!(f.get(15, 13), Some(RED));
        assert_eq!(f.get(4, 13), Some(BLACK));
    }

    #[test]
    fn flood_fills_a_region_and_stops_at_walls() {
        let mut b = blank(10, 10);
        b.rectangle((2, 2), (7, 7), 1, Fill::Outline, BLACK, RED);
        b.flood(4, 4, RED);
        assert_eq!(count(&b, RED), 16);
        assert_eq!(b.get(0, 0), Some(WHITE));
        // From outside it fills everything but the wall and the red inside.
        b.flood(0, 0, [0, 0, 255, 255]);
        assert_eq!(count(&b, [0, 0, 255, 255]), 100 - 20 - 16);
    }

    #[test]
    fn flood_into_the_same_colour_does_nothing_and_stays_in_bounds() {
        let mut b = blank(6, 6);
        b.flood(1, 1, WHITE);
        b.flood(-1, 0, RED);
        b.flood(0, 99, RED);
        assert_eq!(count(&b, WHITE), 36);
    }

    #[test]
    fn flood_is_not_diagonal() {
        let mut b = blank(3, 3);
        for (x, y) in [(1, 0), (0, 1)] {
            b.put(x, y, BLACK);
        }
        b.flood(0, 0, RED);
        assert_eq!(count(&b, RED), 1);
    }

    #[test]
    fn flips_and_turns() {
        let mut b = blank(3, 2);
        b.put(0, 0, RED);
        let mut h = b.clone();
        h.flip_horizontal();
        assert_eq!(h.get(2, 0), Some(RED));
        let mut v = b.clone();
        v.flip_vertical();
        assert_eq!(v.get(0, 1), Some(RED));
        let mut r = b.clone();
        r.rotate_clockwise();
        assert_eq!((r.width, r.height), (2, 3));
        assert_eq!(r.get(1, 0), Some(RED));
        for _ in 0..3 {
            r.rotate_clockwise();
        }
        assert_eq!(r, b);
    }

    #[test]
    fn invert_keeps_alpha() {
        let mut b = Bitmap::new(1, 1, [10, 20, 30, 40]);
        b.invert();
        assert_eq!(b.get(0, 0), Some([245, 235, 225, 40]));
    }

    #[test]
    fn resizing_the_canvas_keeps_the_top_left() {
        let mut b = blank(4, 4);
        b.put(1, 1, RED);
        b.resize_canvas(6, 2, BLACK);
        assert_eq!((b.width, b.height), (6, 2));
        assert_eq!(b.get(1, 1), Some(RED));
        assert_eq!(b.get(5, 0), Some(BLACK));
        b.resize_canvas(0, 0, WHITE);
        assert_eq!((b.width, b.height), (1, 1));
    }

    #[test]
    fn bytes_round_trip() {
        let mut b = blank(2, 2);
        b.put(1, 0, RED);
        assert_eq!(Bitmap::from_rgba(2, 2, &b.to_bytes()), b);
    }
}
