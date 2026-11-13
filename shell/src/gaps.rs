//! Gaps (ADR-0051), shared by the HUD and the menus that match it, from the compositor's config (`[gaps]`), since tiled
//! windows and the bar share them: with gaps on, the bar floats the gap
//! away from the screen's edges, with rounded corners.

use std::path::PathBuf;

/// The largest gap and radius taken, as the compositor has it.
const MOST: i64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Gaps {
    /// The gap, in logical pixels; 0 when gaps are off.
    pub gap: i32,
    /// The corners' radius while the bar floats.
    pub radius: f32,
}

impl Gaps {
    pub fn floating(self) -> bool {
        self.gap > 0
    }

    /// The bar's layer-shell margin on its `top` or bottom edge: top,
    /// right, bottom, left.
    pub fn margin(self, top: bool) -> (i32, i32, i32, i32) {
        let g = self.gap;
        if top { (g, g, 0, g) } else { (0, g, g, g) }
    }
}

/// The compositor's config file.
pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-compositor").join("config.toml"))
}

/// `[gaps]` from the compositor config's text; off for what's missing or
/// wrong, as the compositor reads it.
pub fn parse(text: &str) -> Gaps {
    let Some(table) = text
        .parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("gaps").and_then(|g| g.as_table()).cloned())
    else {
        return Gaps::default();
    };
    if table.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        return Gaps::default();
    }
    let number = |key: &str, default: i64| {
        table
            .get(key)
            .and_then(|v| v.as_integer())
            .filter(|n| (0..=MOST).contains(n))
            .unwrap_or(default)
    };
    Gaps {
        gap: number("size", 8) as i32,
        radius: number("radius", 8) as f32,
    }
}

pub fn read() -> Gaps {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_are_off_unless_enabled() {
        assert_eq!(parse(""), Gaps::default());
        assert_eq!(parse("[gaps]\nsize = 12\n"), Gaps::default());
        assert_eq!(parse("[gaps\nbroken"), Gaps::default());
        let on = parse("[gaps]\nenabled = true\nsize = 12\nradius = 0\n");
        assert_eq!(
            on,
            Gaps {
                gap: 12,
                radius: 0.0
            }
        );
        assert_eq!(
            parse("[gaps]\nenabled = true\nsize = 900\n"),
            Gaps {
                gap: 8,
                radius: 8.0
            },
            "out of range is the default"
        );
    }

    #[test]
    fn the_margin_leaves_the_screens_edge_and_sides() {
        let g = Gaps {
            gap: 10,
            radius: 6.0,
        };
        assert_eq!(g.margin(false), (0, 10, 10, 10));
        assert_eq!(g.margin(true), (10, 10, 0, 10));
    }
}
