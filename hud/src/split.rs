//! The bar split into its parts (experimental, `layout = "split"`): the
//! launcher button, the window list and the tray, each on a surface of its
//! own (`launcher_at`, `windows_at`, `tray_at`), at the start, middle or
//! end of any edge: a row along the top or bottom, a column up a side.
//! Parts stack rather than overlap where they can: a column at a side's
//! end sits past a row in that corner, and a part sharing the launcher's
//! spot lines up after it. The bar draws the window list itself and runs the
//! other parts as `patina-hud --section`, which end when its pipe to them
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

use crate::config::{Along, Config, Position, Side, Spot, Visibility};
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
            Section::Launcher => config.launcher_at,
            Section::Windows => Some(config.windows_at),
            Section::Tray => Some(config.tray_at),
            Section::Media => media_apart(config),
            Section::Reserve(_) => None,
        }
    }
}

impl Section {
    /// Whether the element is shown, hides at its edge or isn't there.
    pub fn mode(self, config: &Config) -> Visibility {
        match self {
            Section::Launcher if config.launcher_at.is_none() => Visibility::Hidden,
            Section::Launcher => config.launcher_mode,
            Section::Windows => config.windows_mode,
            Section::Tray => config.tray_mode,
            Section::Media => config.media_mode,
            Section::Reserve(_) => Visibility::Always,
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

/// Each part on a surface of its own, and where it is: the launcher only
/// when it has a spot.
fn parts(config: &Config) -> Vec<(Section, Spot)> {
    let mut parts = shown_parts(config);
    // An element that hides, or isn't there, keeps no space at its edge.
    parts.retain(|(section, _)| section.mode(config) == Visibility::Always);
    parts
}

/// Every element that has a spot, hiding or not.
fn shown_parts(config: &Config) -> Vec<(Section, Spot)> {
    let mut parts = Vec::new();
    parts.extend(config.launcher_at.map(|spot| (Section::Launcher, spot)));
    parts.push((Section::Windows, config.windows_at));
    parts.push((Section::Tray, config.tray_at));
    parts.extend(media_apart(config).map(|spot| (Section::Media, spot)));
    parts
}

/// How thick `section` is across its edge, in logical pixels: its own
/// `*_height`, else the bar's `height`.
pub fn thickness(section: Section, config: &Config) -> u32 {
    match section {
        Section::Launcher => config.launcher_height,
        Section::Windows => config.windows_height,
        Section::Tray | Section::Media => config.tray_height,
        Section::Reserve(_) => None,
    }
    .unwrap_or(config.height)
    .max(1)
}

/// The two edges a corner spot meets, or just its own for a spot in the
/// middle of an edge.
fn edges(spot: Spot) -> Vec<Side> {
    let across = match (spot.side(), spot.along()) {
        (_, Along::Middle) => return vec![spot.side()],
        (Side::Top | Side::Bottom, Along::Start) => Side::Left,
        (Side::Top | Side::Bottom, _) => Side::Right,
        (_, Along::Start) => Side::Top,
        (_, _) => Side::Bottom,
    };
    vec![spot.side(), across]
}

/// Whether the launcher, the one square part, is in a corner and stays
/// there. Whatever is along either of its edges already keeps that space,
/// as it's as far as the corner goes, so it takes no part in what's kept
/// (see `kept`). A column spot with a part in the row of its corner stacks
/// past that row (see `placement`): it's out of the corner then, over the
/// windows' space, and keeps its edge like any other part.
fn in_corner(config: &Config, section: Section, spot: Spot) -> bool {
    section == Section::Launcher
        && spot.along() != Along::Middle
        && !spot
            .corner_row()
            .is_some_and(|row| parts(config).iter().any(|(_, at)| *at == row))
}

/// How much of `side` is kept, and so how far windows stay from it, as
/// thick as the thickest part along it, or none when no part is. A launcher
/// in a corner keeps nothing of its own, but is as thick as the edges it
/// meets if something else keeps them, so it never covers a window.
fn kept(config: &Config, side: Side) -> Option<u32> {
    let all = parts(config);
    let mut along = all
        .iter()
        .filter(|(section, spot)| !in_corner(config, *section, *spot) && spot.side() == side)
        .map(|(section, _)| thickness(*section, config))
        .peekable();
    along.peek()?;
    let mut most = along.max()?;
    for (section, spot) in &all {
        if in_corner(config, *section, *spot) && edges(*spot).contains(&side) {
            most = most.max(thickness(*section, config));
        }
    }
    Some(most)
}

/// The edges the parts keep, each once.
pub fn reserved(config: &Config) -> Vec<Side> {
    let mut sides = Vec::new();
    for (section, spot) in parts(config) {
        if in_corner(config, section, spot) {
            continue;
        }
        let side = spot.side();
        if !sides.contains(&side) {
            sides.push(side);
        }
    }
    sides
}

/// Where the transparent strip of a part's surface is (top, right, bottom,
/// left): along its inner side, so what hides leaves the strip at the
/// screen's edge, for the pointer to touch.
pub fn hot_trim(side: Side) -> (i32, i32, i32, i32) {
    let hot = crate::hide::HOT;
    match side {
        Side::Bottom => (hot, 0, 0, 0),
        Side::Top => (0, 0, hot, 0),
        Side::Left => (0, hot, 0, 0),
        Side::Right => (0, 0, 0, hot),
    }
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
    /// How much of the surface, across its edge, is the strip a part hides
    /// into; 0 for the whole bar and the reserves.
    pub hot: f32,
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
        // The strip is nothing to open past.
        let surface = match self.opens {
            "left" | "right" => Vec2::new(surface.x - self.hot, surface.y),
            _ => Vec2::new(surface.x, surface.y - self.hot),
        };
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
            hot: 0.0,
            x: Along::Start,
            y: if top { Along::Start } else { Along::End },
        };
    };
    if let Section::Reserve(side) = section {
        // A pixel-thin strip along the whole edge, keeping as much as the
        // thickest part along it and its gap take.
        let (anchor, size) = match side {
            Side::Top => (Anchor::TOP | Anchor::LEFT | Anchor::RIGHT, (0, 1)),
            Side::Bottom => (Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT, (0, 1)),
            Side::Left => (Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM, (1, 0)),
            Side::Right => (Anchor::RIGHT | Anchor::TOP | Anchor::BOTTOM, (1, 0)),
        };
        return Placement {
            anchor,
            margin: (0, 0, 0, 0),
            exclusive_zone: kept(config, side).map_or(0, |t| t as i32) + g,
            size,
            opens: side.name(),
            hot: 0.0,
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
    // column ends in (as thick as the thickest there), and the launcher,
    // when they share the spot.
    let mut before = 0;
    if let Some(row) = spot.corner_row() {
        let thickest = parts(config)
            .iter()
            .filter(|(_, at)| *at == row)
            .map(|(s, _)| thickness(*s, config))
            .max();
        if let Some(t) = thickest {
            before += t as i32 + g;
        }
    }
    if section != Section::Launcher && config.launcher_at == Some(spot) {
        before += thickness(Section::Launcher, config) as i32 + g;
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
    let t = thickness(section, config);
    let hot = crate::hide::HOT as u32;
    let size = match (section, spot.vertical()) {
        (Section::Launcher, true) => (t + hot, t),
        (Section::Launcher, false) => (t, t + hot),
        (_, true) => (t + hot, 200),
        (_, false) => (240, t + hot),
    };
    Placement {
        anchor,
        margin,
        // Kept by the reserve sections instead (see above).
        exclusive_zone: -1,
        size,
        opens: spot.side().name(),
        hot: hot as f32,
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

/// `placement`'s margin for `section` when it is `out` of the way: 1 in
/// place, 0 off the screen but for the sliver at its edge. Only the margin
/// of the edge it sits on moves.
pub fn slid_margin(
    place: &Placement,
    section: Section,
    config: &Config,
    gaps: Gaps,
    out: f32,
    sliver: i32,
) -> (i32, i32, i32, i32) {
    let Some(spot) = section.spot(config) else {
        return place.margin;
    };
    let off = thickness(section, config) as i32 + gaps.gap + crate::hide::HOT - sliver;
    let moved = ((1.0 - out.clamp(0.0, 1.0)) * off as f32).round() as i32;
    let (mut top, mut right, mut bottom, mut left) = place.margin;
    match spot.side() {
        Side::Top => top -= moved,
        Side::Right => right -= moved,
        Side::Bottom => bottom -= moved,
        Side::Left => left -= moved,
    }
    (top, right, bottom, left)
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
    const GAMELESS: Gaps = Gaps {
        gap: 0,
        radius: 0.0,
    };

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
        let at = p.popup(egui::pos2(10.0, 5.0), egui::vec2(400.0, 51.0), Some(OUT));
        assert_eq!((at.edge, at.x, at.y), ("bottom", 770.0, 53.0));
    }

    #[test]
    fn a_column_opens_out_to_the_side() {
        let c = config("layout = \"split\"\nwindows_at = \"center-left\"");
        let p = placement(Some(Section::Windows), &c, GAPS);
        assert_eq!(p.anchor, Anchor::LEFT);
        let at = p.popup(egui::pos2(20.0, 30.0), egui::vec2(51.0, 200.0), Some(OUT));
        // Past the column, level with the icon; the column is centred.
        assert_eq!((at.edge, at.x, at.y), ("left", 53.0, 470.0));
    }

    #[test]
    fn any_part_goes_in_any_corner() {
        let c = config("layout = \"split\"\ntray_at = \"top-right\"\nlauncher_at = \"top-left\"");
        let tray = placement(Some(Section::Tray), &c, GAPS);
        assert_eq!(tray.anchor, Anchor::TOP | Anchor::RIGHT);
        let at = tray.popup(egui::pos2(10.0, 5.0), egui::vec2(200.0, 51.0), Some(OUT));
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
        let at = column.popup(egui::pos2(20.0, 30.0), egui::vec2(51.0, 120.0), Some(OUT));
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
        // The launcher's bottom-left corner keeps nothing of its own.
        assert_eq!(reserved(&c), [Side::Right, Side::Top]);
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

    #[test]
    fn a_launcher_in_a_corner_keeps_no_edge_of_its_own() {
        // The defaults: the launcher is bottom-left, the tray bottom-right.
        // The bottom is kept for the tray, so the launcher adds nothing.
        let c = config("layout = \"split\"\nwindows_at = \"top\"\ntray_at = \"top-right\"");
        assert_eq!(
            reserved(&c),
            [Side::Top],
            "nothing keeps the bottom or left"
        );
        let c = config("layout = \"split\"\nwindows_at = \"left\"\ntray_at = \"top-right\"");
        assert_eq!(
            reserved(&c),
            [Side::Left, Side::Top],
            "windows fill to the bottom: the left is kept top to bottom, not the bottom"
        );
    }

    #[test]
    fn a_launcher_in_the_middle_of_an_edge_keeps_it() {
        let c = config("layout = \"split\"\nlauncher_at = \"right\"");
        assert!(reserved(&c).contains(&Side::Right));
    }

    #[test]
    fn a_corner_launcher_is_as_thick_as_the_edges_kept_beside_it() {
        let c = config("layout = \"split\"\nlauncher_height = 64\ntray_height = 24");
        // Bottom: the tray's 24 and the launcher's 64, which sits in the corner.
        assert_eq!(kept(&c, Side::Bottom), Some(64));
        assert_eq!(kept(&c, Side::Left), None, "nothing keeps the left");
        let keep = placement(Some(Section::Reserve(Side::Bottom)), &c, GAPS);
        assert_eq!(keep.exclusive_zone, 64 + 8);
    }

    #[test]
    fn parts_are_as_thick_as_they_are_told() {
        let c = config("layout = \"split\"\nwindows_height = 72\ntray_height = 24");
        assert_eq!(thickness(Section::Windows, &c), 72);
        assert_eq!(thickness(Section::Tray, &c), 24);
        assert_eq!(thickness(Section::Launcher, &c), 41);
        // The surface is the element and the strip it hides into.
        assert_eq!(placement(Some(Section::Windows), &c, GAPS).size.1, 72 + 10);
        assert_eq!(placement(Some(Section::Tray), &c, GAPS).size.1, 24 + 10);
        // The bottom is kept as far as the dock reaches.
        assert_eq!(
            placement(Some(Section::Reserve(Side::Bottom)), &c, GAPS).exclusive_zone,
            72 + 8
        );
    }

    #[test]
    fn without_a_launcher_nothing_runs_it_or_stacks_past_it() {
        let c = config("layout = \"split\"\nlauncher_at = \"none\"\nwindows_at = \"left-bottom\"");
        assert_eq!(c.launcher_at, None);
        let column = placement(Some(Section::Windows), &c, GAPS);
        assert_eq!(
            column.margin,
            (0, 0, 8, 8),
            "at the corner: nothing's in it"
        );
        assert_eq!(reserved(&c), [Side::Left, Side::Bottom]);
    }

    #[test]
    fn a_launcher_stacked_above_a_row_keeps_its_side() {
        // `left-bottom` with a window list along the bottom-left: the
        // launcher sits above the row, out of the corner.
        let c = config(
            "layout = \"split\"\nlauncher_at = \"left-bottom\"\nwindows_at = \"bottom-left\"",
        );
        let launcher = placement(Some(Section::Launcher), &c, GAPS);
        assert_eq!(launcher.margin, (0, 0, 8 + 41 + 8, 8));
        assert!(reserved(&c).contains(&Side::Left));
        assert_eq!(kept(&c, Side::Left), Some(41));
        // With nothing in the corner's row it's in the corner, and keeps nothing.
        let c = config(
            "layout = \"split\"\nlauncher_at = \"left-bottom\"\nwindows_at = \"top\"\ntray_at = \"top-right\"",
        );
        assert!(!reserved(&c).contains(&Side::Left));
    }

    #[test]
    fn an_element_that_hides_keeps_no_space() {
        let c = config("launcher_mode = \"autohide\"\ntray_mode = \"hidden\"");
        assert_eq!(Section::Launcher.mode(&c), Visibility::Autohide);
        assert_eq!(Section::Tray.mode(&c), Visibility::Hidden);
        // Only the window list is left to keep the bottom edge.
        assert_eq!(reserved(&c), vec![Side::Bottom]);
        let c = config("windows_mode = \"autohide\"\nlauncher_mode = \"hidden\"\ntray_mode = \"autohide\"");
        assert!(reserved(&c).is_empty());
    }

    #[test]
    fn a_hidden_element_slides_off_its_own_edge() {
        let c = config("windows_at = \"bottom\"\nheight = 40");
        let p = placement(Some(Section::Windows), &c, GAMELESS);
        let (top, right, bottom, left) = slid_margin(&p, Section::Windows, &c, GAMELESS, 0.0, crate::hide::HOT);
        assert_eq!((top, right, left), (p.margin.0, p.margin.1, p.margin.3));
        assert_eq!(bottom, p.margin.2 - 40, "all but the strip");
        assert_eq!(slid_margin(&p, Section::Windows, &c, GAMELESS, 1.0, crate::hide::HOT), p.margin);
        let c = config("tray_at = \"right-top\"\nheight = 40");
        let p = placement(Some(Section::Tray), &c, GAMELESS);
        assert_eq!(slid_margin(&p, Section::Tray, &c, GAMELESS, 0.0, 0).1, p.margin.1 - 50);
    }
}
