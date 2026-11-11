//! Rounded bottom corners for a window that draws square ones (ADR-0133): Chromium's `--app`
//! window, which unlike the WebKit host can't be told to round its own. The renderer is
//! software (pixman) and has no shader to clip with, so a window's surface elements are wrapped
//! to draw only the rows' worth of a rounded rectangle: the damage they're asked to draw is cut
//! to a body and one strip per row of each corner, and the corners are reported as not opaque so
//! what's behind them is drawn too. The edge is a stair; `frames` draws the one-pixel border over
//! it as an anti-aliased arc, which hides the steps.

use smithay::{
    backend::renderer::{
        Renderer,
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Physical, Point, Rectangle, Scale, Transform},
};

/// How far in from the corner box's side the rounded edge is, for each of `radius` rows going
/// down the corner (the box's top row has the least).
pub fn insets(radius: i32) -> Vec<i32> {
    (0..radius)
        .map(|row| {
            let dy = f64::from(row) + 0.5;
            let dx = (f64::from(radius * radius) - dy * dy).max(0.0).sqrt();
            (f64::from(radius) - dx).round() as i32
        })
        .collect()
}

/// The rectangles, from the window's top left, that a `w` by `h` window with bottom corners
/// rounded by `radius` covers: its body, then a strip for each run of rows of the corners that
/// are inset the same.
pub fn shape(w: i32, h: i32, radius: i32) -> Vec<Rectangle<i32, Physical>> {
    let radius = radius.clamp(0, (w / 2).min(h));
    let mut rects = vec![Rectangle::new((0, 0).into(), (w, h - radius).into())];
    let insets = insets(radius);
    let mut row = 0;
    while row < insets.len() {
        let inset = insets[row];
        let run = insets[row..].iter().take_while(|i| **i == inset).count();
        rects.push(Rectangle::new(
            (inset, h - radius + row as i32).into(),
            (w - 2 * inset, run as i32).into(),
        ));
        row += run;
    }
    rects.retain(|r| !r.is_empty());
    rects
}

/// `E`, the surface element of a window, drawn clipped to the window's rounded shape.
pub struct Rounded<E> {
    inner: E,
    /// The window's rectangle on the output.
    window: Rectangle<i32, Physical>,
    radius: i32,
}

impl<E> Rounded<E> {
    pub fn new(inner: E, window: Rectangle<i32, Physical>, radius: i32) -> Self {
        Self {
            inner,
            window,
            radius,
        }
    }
}

impl<E: Element> Element for Rounded<E> {
    fn id(&self) -> &Id {
        self.inner.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.inner.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }

    fn transform(&self) -> Transform {
        self.inner.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }

    /// The inner element's, less the corners: they're drawn through, so what's under them has
    /// to be drawn first.
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        let geometry = self.inner.geometry(scale);
        let r = self.radius;
        let top = self.window.loc.y + self.window.size.h - r - geometry.loc.y;
        let corners = [
            Rectangle::new(
                (self.window.loc.x - geometry.loc.x, top).into(),
                (r, r).into(),
            ),
            Rectangle::new(
                (
                    self.window.loc.x + self.window.size.w - r - geometry.loc.x,
                    top,
                )
                    .into(),
                (r, r).into(),
            ),
        ];
        let kept: Vec<_> = self
            .inner
            .opaque_regions(scale)
            .iter()
            .flat_map(|region| region.subtract_rects(corners))
            .collect();
        OpaqueRegions::from_slice(&kept)
    }

    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }

    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: Renderer, E: RenderElement<R>> RenderElement<R> for Rounded<E> {
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        // The damage is relative to `dst`; the shape to the window.
        let origin = self.window.loc - dst.loc;
        let shape: Vec<_> = shape(self.window.size.w, self.window.size.h, self.radius)
            .into_iter()
            .map(|rect| Rectangle::new(rect.loc + origin, rect.size))
            .collect();
        let clipped: Vec<_> = damage
            .iter()
            .flat_map(|d| shape.iter().filter_map(|s| d.intersection(*s)))
            .collect();
        if clipped.is_empty() {
            return Ok(());
        }
        self.inner.draw(frame, src, dst, &clipped, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        // Not for direct scanout: the corners have to be cut.
        let _ = renderer;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_is_inset_most_at_its_bottom() {
        let insets = super::insets(15);
        assert_eq!(insets.len(), 15);
        assert!(
            insets[0] <= 1,
            "the top row of the corner is almost full width"
        );
        assert!(insets[14] >= 10, "the bottom row is cut a long way in");
        assert!(
            insets.windows(2).all(|w| w[0] <= w[1]),
            "never back out going down"
        );
        assert_eq!(super::insets(0), Vec::<i32>::new());
    }

    #[test]
    fn the_shape_covers_the_body_and_a_stair_of_strips() {
        let shape = shape(100, 80, 15);
        assert_eq!(
            shape[0],
            Rectangle::new((0, 0).into(), (100, 65).into()),
            "the body"
        );
        let bottom = shape.last().unwrap();
        assert_eq!(
            bottom.loc.y + bottom.size.h,
            80,
            "down to the window's bottom"
        );
        assert!(bottom.size.w < 100 - 2 * 8, "narrower at the bottom");
        // No row is covered twice, and every row of the window is covered.
        let rows: i32 = shape.iter().map(|r| r.size.h).sum();
        assert_eq!(rows, 80);
        // A window smaller than its corners still makes a sane shape.
        assert!(!super::shape(10, 6, 15).is_empty());
    }
}
