//! The bar split into its parts (experimental, `layout = "split"`): the
//! launcher button, the window list and the tray, each on a surface of its
//! own (`launcher_at`, `windows_at`, `tray_at`), at the start, middle or
//! end of any edge: a row along the top or bottom, a column up a side.
//! Parts stack rather than overlap where they can: a column at a side's
//! end sits past a row in that corner, and a part sharing the launcher's
//! spot lines up after it. The bar draws the window list itself and runs the
//! other parts as `patina-bar --section`, which end when its pipe to them
//! closes.
//!
//! The parts keep no space of their own: a corner can't keep the edge it's
//! along (the compositor takes a surface in a corner as keeping the side
//! edge). Instead, each edge with a part on it is kept by a "reserve"
//! section, a pixel-thin surface along the whole edge that takes no
//! clicks.
//!
//! Where each surface sits, and so where its menus, tooltips and the
//! pieces it opens go on the screen, is here, for the whole bar too.

use egui::{Pos2, Vec2};
use patina_shell::Anchor;

use crate::config::{Along, Config, Position, Side, Spot};
use crate::gaps::Gaps;

/// A part of the split bar, or the keeper of an edge's space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Launcher,
    Windows,
    Tray,
    /// The media controls, apart from the tray (ADR-0101).
    Media,
    Reserve(Side),
}

impl Section {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "launcher" => Section::Launcher,
            "windows" => Section::Windows,
            "tray" => Section::Tray,
            "media" => Section::Media,
            "reserve-top" => Section::Reserve(Side::Top),
            "reserve-bottom" => Section::Reserve(Side::Bottom),
            "reserve-left" => Section::Reserve(Side::Left),
            "reserve-right" => Section::Reserve(Side::Right),
            _ => return None,
        })
    }

    pub fn name(self) -> String {
        match self {
            Section::Launcher => "launcher".into(),
            Section::Windows => "windows".into(),
            Section::Tray => "tray".into(),
            Section::Media => "media".into(),
            Section::Reserve(side) => format!("reserve-{}", side.name()),
        }
    }

    /// Where the part goes, as `config` says.
    pub fn spot(self, config: &Config) -> Option<Spot> {
        match self {
            Section::Launcher => Some(config.launcher_at),
            Section::Windows => Some(config.windows_at),
            Section::Tray => Some(config.tray_at),
            Section::Media => media_apart(config),
            Section::Reserve(_) => None,
        }
    }
}

/// Where the media controls sit on a part of their own: `media_at`, when
/// the bar has them and it isn't the tray's spot, where they would only
/// cover the tray. Otherwise they're in the tray's part.
pub fn media_apart(config: &Config) -> Option<Spot> {
    config
        .media_at
        .filter(|spot| *spot != config.tray_at && config.has_module("media"))
}

/// Where each part is, of those on a surface of their own.
fn spots(config: &Config) -> Vec<Spot> {
    let mut spots = vec![config.launcher_at, config.windows_at, config.tray_at];
    spots.extend(media_apart(config));
    spots
}

/// The edges the parts are along, each to be kept once.
pub fn reserved(config: &Config) -> Vec<Side> {
    let mut sides = Vec::new();
    for spot in spots(config) {
        let side = spot.side();
        if !sides.contains(&side) {
            sides.push(side);
        }
    }
    sides
}

/// How far past a surface's edge its menus and tooltips open.
pub const SPACE: f32 = 4.0;

/// Where a menu, tooltip or piece opens, as patina-menu takes it: from
/// `edge` (top or bottom: X from the left and Y from that edge; left or
/// right: X from that edge and Y from the top), and how far it is from the
/// screen's far side along it (right or bottom), when the output's size is
/// known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Popup {
    pub edge: &'static str,
    pub x: f32,
    pub y: f32,
    pub far: Option<f32>,
}

impl Popup {
    /// As `PATINA_OPEN_AT` has it: `edge,x,y[,far]`.
    pub fn env(&self) -> String {
        match self.far {
            Some(far) => format!("{},{:.0},{:.0},{:.0}", self.edge, self.x, self.y, far),
            None => format!("{},{:.0},{:.0}", self.edge, self.x, self.y),
        }
    }
}

/// How a surface sits on its output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub anchor: Anchor,
    /// Top, right, bottom, left.
    pub margin: (i32, i32, i32, i32),
    pub exclusive_zone: i32,
    /// Its first size; a part then sizes itself to what it shows.
    pub size: (u32, u32),
    /// Which way its menus and tooltips open: top, bottom, left or right.
    pub opens: &'static str,
    x: Along,
    y: Along,
}

impl Placement {
    /// Where the surface's top left is, `surface` big on an output `out`
    /// big (the margins alone when the output's size isn't known).
    fn origin(&self, surface: Vec2, out: Option<Vec2>) -> Pos2 {
        let (top, right, bottom, left) = self.margin;
        let at =
            |along: Along, start: i32, end: i32, size: f32, out: Option<f32>| match (along, out) {
                (Along::Start, _) | (_, None) => start as f32,
                (Along::Middle, Some(o)) => (o - size) / 2.0,
                (Along::End, Some(o)) => o - end as f32 - size,
            };
        Pos2::new(
            at(self.x, left, right, surface.x, out.map(|o| o.x)),
            at(self.y, top, bottom, surface.y, out.map(|o| o.y)),
        )
    }

    /// Where `point`, in a surface `surface` big, opens a menu, tooltip or
    /// piece, on an output `output` big (if the compositor said).
    pub fn popup(&self, point: Pos2, surface: Vec2, output: Option<Vec2>) -> Popup {
        let (top, right, bottom, left) = self.margin;
        let origin = self.origin(surface, output);
        match self.opens {
            "left" | "right" => {
                let side = if self.opens == "left" { left } else { right };
                let y = origin.y + point.y;
                Popup {
                    edge: self.opens,
                    x: side as f32 + surface.x + SPACE,
                    y,
                    far: output.map(|o| o.y - y),
                }
            }
            edge => {
                let from = if edge == "top" { top } else { bottom };
                let x = origin.x + point.x;
                Popup {
                    edge,
                    x,
                    y: from as f32 + surface.y + SPACE,
                    far: output.map(|o| o.x - x),
                }
            }
        }
    }
}

/// Where `section` goes (the whole bar for `None`), with `config` and
/// `gaps`.
pub fn placement(section: Option<Section>, config: &Config, gaps: Gaps) -> Placement {
    let h = config.height.max(1);
    let (hi, g) = (h as i32, gaps.gap);
    let Some(section) = section else {
        let top = config.position == Position::Top;
        let side = if top { Anchor::TOP } else { Anchor::BOTTOM };
        return Placement {
            anchor: side | Anchor::LEFT | Anchor::RIGHT,
            margin: gaps.margin(top),
            exclusive_zone: hi,
            size: (0, h),
            opens: if top { "top" } else { "bottom" },
            x: Along::Start,
            y: if top { Along::Start } else { Along::End },
        };
    };
    if let Section::Reserve(side) = section {
        // A pixel-thin strip along the whole edge, keeping as much as a
        // part and its gap take.
        let (anchor, size) = match side {
            Side::Top => (Anchor::TOP | Anchor::LEFT | Anchor::RIGHT, (0, 1)),
            Side::Bottom => (Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, (0, 1)),
            Side::Left => (Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM, (1, 0)),
            Side::Right => (Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM, (1, 0)),
        };
        return Placement {
            anchor,
            margin: (0, 0, 0, 0),
            exclusive_zone: hi + g,
            size,
            opens: side.name(),
            x: Along::Start,
            y: Along::Start,
        };
    }
    let spot = section.spot(config).expect("a part has a spot");
    let mut anchor = Anchor::empty();
    let mut margin = (0, 0, 0, 0);
    match spot.y() {
        Along::Start => {
            anchor |= Anchor::TOP;
            margin.0 = g;
        }
        Along::End => {
            anchor |= Anchor::BOTTOM;
            margin.2 = g;
        }
        Along::Middle => {}
    }
    match spot.x() {
        Along::Start => {
            anchor |= Anchor::LEFT;
            margin.3 = g;
        }
        Along::End => {
            anchor |= Anchor::RIGHT;
            margin.1 = g;
        }
        Along::Middle => {}
    }
    // Past what's before it at its end of the edge: a row in the corner a
    // column ends in, and the launcher, when they share the spot. Both are
    // as thick as the bar.
    let mut before = 0;
    if spot
        .corner_row()
        .is_some_and(|row| spots(config).contains(&row))
    {
        before += hi + g;
    }
    if section != Section::Launcher && config.launcher_at == spot {
        before += hi + g;
    }
    if spot.along() != Along::Middle {
        let end = match (spot.vertical(), spot.along()) {
            (false, Along::Start) => &mut margin.3,
            (false, _) => &mut margin.1,
            (true, Along::Start) => &mut margin.0,
            (true, _) => &mut margin.2,
        };
        *end = g + before;
    }
    let size = match (section, spot.vertical()) {
        (Section::Launcher, _) => (h, h),
        (_, true) => (h, 200),
        (_, false) => (240, h),
    };
    Placement {
        anchor,
        margin,
        // Kept by the reserve sections instead (see above).
        exclusive_zone: -1,
        size,
        opens: spot.side().name(),
        x: spot.x(),
        y: spot.y(),
    }
}

/// The modules `section` draws, of those `config` has: the launcher, the
/// window list, or the rest (the tray, the clock, the power button).
pub fn modules(section: Section, config: &Config) -> Vec<String> {
    match section {
        Section::Launcher => vec!["launcher".into()],
        Section::Windows => vec!["windows".into()],
        Section::Media if media_apart(config).is_some() => vec!["media".into()],
        Section::Media | Section::Reserve(_) => Vec::new(),
        Section::Tray => config
            .left
            .iter()
            .chain(&config.center)
            .chain(&config.right)
            .filter(|m| !matches!(m.as_str(), "launcher" | "windows"))
            .filter(|m| m.as_str() != "media" || media_apart(config).is_none())
            // dwl's, which say nothing under Patina's compositor.
            .filter(|m| !matches!(m.as_str(), "tags" | "layout" | "title" | "appid"))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(text: &str) -> Config {
        crate::config::parse(text).0
    }

    const GAPS: Gaps = Gaps {
        gap: 8,
        radius: 8.0,
    };
    const OUT: Vec2 = Vec2::new(1920.0, 1080.0);

    #[test]
    fn the_whole_bars_menus_open_past_its_edge_as_before() {
        let p = placement(None, &config(""), GAPS);
        let at = p.popup(egui::pos2(100.0, 10.0), egui::vec2(1904.0, 41.0), Some(OUT));
        assert_eq!((at.edge, at.x, at.y), ("bottom", 108.0, 53.0));
        assert_eq!(at.far, Some(1812.0));
        assert_eq!(at.env(), "bottom,108,53,1812");
    }

    #[test]
    fn a_centred_dock_finds_itself_from_the_outputs_width() {
        let p = placement(Some(Section::Windows), &config("layout = \"split\""), GAPS);
        assert_eq!(p.anchor, Anchor::BOTTOM);
        assert_eq!(p.exclusive_zone, -1, "kept by the reserve instead");
        let at = p.popup(egui::pos2(10.0, 5.0), egui::vec2(400.0, 41.0), Some(OUT));
        assert_eq!((at.edge, at.x, at.y), ("bottom", 770.0, 53.0));
    }

    #[test]
    fn a_column_opens_out_to_the_side() {
        let c = config("layout = \"split\"\nwindows_at = \"center-left\"");
        let p = placement(Some(Section::Windows), &c, GAPS);
        assert_eq!(p.anchor, Anchor::LEFT);
        let at = p.popup(egui::pos2(20.0, 30.0), egui::vec2(41.0, 200.0), Some(OUT));
        // Past the column, level with the icon; the column is centred.
        assert_eq!((at.edge, at.x, at.y), ("left", 53.0, 470.0));
    }

    #[test]
    fn any_part_goes_in_any_corner() {
        let c = config("layout = \"split\"\ntray_at = \"top-right\"\nlauncher_at = \"top-left\"");
        let tray = placement(Some(Section::Tray), &c, GAPS);
        assert_eq!(tray.anchor, Anchor::TOP | Anchor::RIGHT);
        let at = tray.popup(egui::pos2(10.0, 5.0), egui::vec2(200.0, 41.0), Some(OUT));
        assert_eq!((at.edge, at.x, at.y), ("top", 1722.0, 53.0));
        let launcher = placement(Some(Section::Launcher), &c, GAPS);
        assert_eq!(launcher.anchor, Anchor::TOP | Anchor::LEFT);
    }

    #[test]
    fn a_column_stacks_above_the_launcher_in_its_corner() {
        let c = config("layout = \"split\"\nwindows_at = \"left-bottom\"");
        let column = placement(Some(Section::Windows), &c, GAPS);
        assert_eq!(column.anchor, Anchor::LEFT | Anchor::BOTTOM);
        // Past the launcher at bottom-left: its 41, and a gap either side.
        assert_eq!(column.margin, (0, 0, 8 + 41 + 8, 8));
        let at = column.popup(egui::pos2(20.0, 30.0), egui::vec2(41.0, 120.0), Some(OUT));
        assert_eq!(
            (at.edge, at.x, at.y),
            ("left", 53.0, 1080.0 - 57.0 - 120.0 + 30.0)
        );
    }

    #[test]
    fn a_part_sharing_the_launchers_spot_lines_up_after_it() {
        let c = config(
            "layout = \"split\"\nlauncher_at = \"left-bottom\"\nwindows_at = \"left-bottom\"",
        );
        let launcher = placement(Some(Section::Launcher), &c, GAPS);
        assert_eq!(launcher.margin, (0, 0, 8, 8));
        let column = placement(Some(Section::Windows), &c, GAPS);
        assert_eq!(column.margin, (0, 0, 57, 8));
        assert_eq!(
            reserved(&c),
            [Side::Left, Side::Bottom],
            "the tray's still at the bottom"
        );
    }

    #[test]
    fn each_edge_with_a_part_is_kept_once() {
        let c = config("layout = \"split\"");
        assert_eq!(reserved(&c), [Side::Bottom]);
        let c =
            config("layout = \"split\"\ntray_at = \"top-right\"\nwindows_at = \"center-right\"");
        assert_eq!(reserved(&c), [Side::Bottom, Side::Right, Side::Top]);
        let keep = placement(Some(Section::Reserve(Side::Top)), &c, GAPS);
        assert_eq!(keep.exclusive_zone, 49);
        assert_eq!(keep.size, (0, 1));
    }

    #[test]
    fn the_tray_has_the_rest_of_the_modules() {
        let c = config("");
        assert_eq!(
            modules(Section::Tray, &c),
            ["media", "tray", "clock", "power"]
        );
        assert_eq!(modules(Section::Launcher, &c), ["launcher"]);
    }

    #[test]
    fn the_media_controls_are_in_the_tray_until_given_a_spot() {
        let c = config("layout = \"split\"");
        assert_eq!(media_apart(&c), None);
        assert!(modules(Section::Media, &c).is_empty());
        let c = config("layout = \"split\"\nmedia_at = \"top-right\"");
        assert_eq!(media_apart(&c), Some(Spot::TopRight));
        assert_eq!(modules(Section::Media, &c), ["media"]);
        assert_eq!(modules(Section::Tray, &c), ["tray", "clock", "power"]);
        assert_eq!(reserved(&c), [Side::Bottom, Side::Top]);
        let part = placement(Some(Section::Media), &c, GAPS);
        assert_eq!(part.anchor, Anchor::TOP | Anchor::RIGHT);
    }

    #[test]
    fn media_controls_can_be_turned_off_or_kept_off_the_trays_spot() {
        let off =
            config("layout = \"split\"\nmedia_at = \"top-right\"\nright = [\"tray\", \"clock\"]");
        assert_eq!(media_apart(&off), None);
        assert_eq!(modules(Section::Tray, &off), ["tray", "clock"]);
        let same = config("layout = \"split\"\nmedia_at = \"bottom-right\"");
        assert_eq!(media_apart(&same), None, "it would cover the tray");
        assert_eq!(modules(Section::Tray, &same)[0], "media");
    }
}
