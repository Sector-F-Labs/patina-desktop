//! How the wallpaper image meets the screen: `wallpaper_fit` in the bar's
//! config, which patina-wallpaper draws by and Settings' Wallpaper page
//! sets. There's no stretching: an image pulled out of its shape never
//! looks right.

/// How the image is laid on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// Scaled to cover the screen, its edges cropped: the default.
    #[default]
    Fill,
    /// Scaled to show all of it, with the skin's colour either side.
    Fit,
    /// At its own size in the middle, cropped if it's bigger.
    Centre,
}

impl Fit {
    pub const ALL: [Fit; 3] = [Fit::Fill, Fit::Fit, Fit::Centre];

    /// Its name in the config.
    pub fn name(self) -> &'static str {
        match self {
            Fit::Fill => "fill",
            Fit::Fit => "fit",
            Fit::Centre => "centre",
        }
    }

    /// The fit named `name`; Fill for an unknown or missing one.
    pub fn from_name(name: Option<&str>) -> Self {
        match name.map(str::trim) {
            Some("fit") => Fit::Fit,
            Some("centre" | "center") => Fit::Centre,
            _ => Fit::Fill,
        }
    }

    /// What Settings calls it.
    pub fn label(self) -> &'static str {
        match self {
            Fit::Fill => "Fill",
            Fit::Fit => "Fit",
            Fit::Centre => "Centre",
        }
    }

    /// What it does, for a tooltip.
    pub fn description(self) -> &'static str {
        match self {
            Fit::Fill => "Covers the screen, cropping the image's edges to its shape",
            Fit::Fit => "Shows all of the image, with the skin's colour either side",
            Fit::Centre => "At the image's own size in the middle of the screen",
        }
    }

    /// Where an `image` of that many pixels goes on a `screen` of that
    /// many: the part of the image shown (x, y, width, height, in its
    /// pixels), and its size on the screen, centred there.
    pub fn layout(self, image: [u32; 2], screen: [u32; 2]) -> ([u32; 4], [u32; 2]) {
        let [iw, ih] = image.map(|v| v.max(1));
        let [sw, sh] = screen.map(|v| v.max(1));
        let centred = |w: u32, h: u32| [(iw - w) / 2, (ih - h) / 2, w, h];
        match self {
            Fit::Fill => {
                let scale = (sw as f64 / iw as f64).max(sh as f64 / ih as f64);
                let w = ((sw as f64 / scale).round() as u32).clamp(1, iw);
                let h = ((sh as f64 / scale).round() as u32).clamp(1, ih);
                (centred(w, h), [sw, sh])
            }
            Fit::Fit => {
                let scale = (sw as f64 / iw as f64).min(sh as f64 / ih as f64);
                let w = ((iw as f64 * scale).round() as u32).clamp(1, sw);
                let h = ((ih as f64 * scale).round() as u32).clamp(1, sh);
                ([0, 0, iw, ih], [w, h])
            }
            Fit::Centre => {
                let (w, h) = (iw.min(sw), ih.min(sh));
                (centred(w, h), [w, h])
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: [u32; 2] = [1920, 1080];

    #[test]
    fn names_round_trip_and_anything_else_fills() {
        for fit in Fit::ALL {
            assert_eq!(Fit::from_name(Some(fit.name())), fit);
        }
        assert_eq!(Fit::from_name(Some("center")), Fit::Centre);
        assert_eq!(Fit::from_name(Some("stretch")), Fit::Fill);
        assert_eq!(Fit::from_name(None), Fit::Fill);
    }

    #[test]
    fn fill_covers_the_screen_cropping_the_sides() {
        // A square image on a wide screen loses its top and bottom.
        let (crop, size) = Fit::Fill.layout([2000, 2000], SCREEN);
        assert_eq!(size, SCREEN);
        assert_eq!(crop, [0, 437, 2000, 1125]);
    }

    #[test]
    fn fit_shows_it_all() {
        let (crop, size) = Fit::Fit.layout([2000, 2000], SCREEN);
        assert_eq!(crop, [0, 0, 2000, 2000]);
        assert_eq!(size, [1080, 1080]);
        // Small images grow to fit too.
        assert_eq!(Fit::Fit.layout([960, 540], SCREEN).1, SCREEN);
    }

    #[test]
    fn centre_keeps_its_size_and_crops_what_doesnt_fit() {
        assert_eq!(
            Fit::Centre.layout([800, 600], SCREEN),
            ([0, 0, 800, 600], [800, 600])
        );
        assert_eq!(
            Fit::Centre.layout([4000, 1000], SCREEN),
            ([1040, 0, 1920, 1000], [1920, 1000])
        );
    }
}
