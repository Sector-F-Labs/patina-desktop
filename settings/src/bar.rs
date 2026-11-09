//! The HUD page (ADR-0050): what the window tabs show, which edge the bar
//! sits on, whether it's one bar or split into its parts and where each
//! goes, and the bar's config file for everything else. All
//! are top-level keys in the bar's config, which the bar follows within a
//! second.

use egui::{RichText, Ui};
use patina_shell::ShadowStyle;

use crate::app::Message;

/// The window tabs' choices: the config's value and what Settings calls it.
const WINDOW_TABS: [(&str, &str); 3] = [
    ("both", "Icons and titles"),
    ("icons", "Icons"),
    ("titles", "Titles"),
];

/// Where a part of the split bar can go: the config's value, what
/// Settings calls it, and where it is on the map, from (0, 0) at the top
/// left to (1, 1). Along the top and bottom a part is a row; up the sides,
/// a column.
const SPOTS: [(&str, &str, (f32, f32)); 12] = [
    ("top-left", "Top edge, left", (0.15, 0.0)),
    ("top-center", "Top edge, centre", (0.5, 0.0)),
    ("top-right", "Top edge, right", (0.85, 0.0)),
    ("bottom-left", "Bottom edge, left", (0.15, 1.0)),
    ("bottom-center", "Bottom edge, centre", (0.5, 1.0)),
    ("bottom-right", "Bottom edge, right", (0.85, 1.0)),
    ("left-top", "Left side, top", (0.0, 0.25)),
    ("left-center", "Left side, middle", (0.0, 0.5)),
    ("left-bottom", "Left side, bottom", (0.0, 0.75)),
    ("right-top", "Right side, top", (1.0, 0.25)),
    ("right-center", "Right side, middle", (1.0, 0.5)),
    ("right-bottom", "Right side, bottom", (1.0, 0.75)),
];

/// The split bar's parts: their config key, what Settings calls them, their
/// mark on the map, and where they go by default.
const PARTS: [(&str, &str, &str, &str); 3] = [
    ("launcher_at", "Launcher", "L", "bottom-left"),
    ("windows_at", "Window list", "W", "bottom-left"),
    ("tray_at", "Tray", "T", "left-bottom"),
];

/// Whether a part is shown: the config's `*_mode` value and what Settings
/// calls it (ADR-0111).
const MODES: [(&str, &str); 3] = [
    ("always", "Always shown"),
    ("autohide", "Hides at its edge"),
    ("hidden", "Hidden"),
];

/// The launcher's value for no button: it has its own shortcut.
const NONE: &str = "none";

/// A spot as the config may say it, older names included.
fn spot(value: &str) -> Option<&'static str> {
    let value = match value {
        "top" => "top-center",
        "bottom" => "bottom-center",
        "left" | "center-left" => "left-center",
        "right" | "center-right" => "right-center",
        v => v,
    };
    SPOTS.iter().map(|(v, ..)| *v).find(|v| *v == value)
}

/// Spot `index`'s point on a map drawn in `inner`.
fn spot_point(index: usize, inner: egui::Rect) -> egui::Pos2 {
    let (x, y) = SPOTS[index].2;
    egui::pos2(
        inner.left() + inner.width() * x,
        inner.top() + inner.height() * y,
    )
}

/// The spot nearest `pos` on a map drawn in `inner`: where a dragged part
/// snaps to.
fn nearest(pos: egui::Pos2, inner: egui::Rect) -> usize {
    (0..SPOTS.len())
        .min_by(|&a, &b| {
            let d = |i| spot_point(i, inner).distance_sq(pos);
            d(a).total_cmp(&d(b))
        })
        .unwrap_or(0)
}

/// A part's mark on the map at spot `index`: a bar along the edge for a
/// row, up it for a column; `nth` of those sharing the spot, after the
/// first.
fn mark_rect(index: usize, nth: usize, inner: egui::Rect) -> egui::Rect {
    let vertical = SPOTS[index].0.starts_with("left") || SPOTS[index].0.starts_with("right");
    let size = if vertical {
        egui::vec2(16.0, 34.0)
    } else {
        egui::vec2(34.0, 16.0)
    };
    let step = if vertical {
        egui::vec2(0.0, -(size.y + 3.0))
    } else {
        egui::vec2(size.x + 3.0, 0.0)
    };
    egui::Rect::from_center_size(spot_point(index, inner) + step * nth as f32, size)
}

/// The thinnest and thickest a part can be made here, in pixels.
const SIZES: std::ops::RangeInclusive<u32> = 16..=160;

/// What the bar's `height` is when the file doesn't say.
const HEIGHT: u32 = 41;

pub struct BarPage {
    /// How thick each part is, in the order of `PARTS`: its `*_height`, or
    /// the bar's `height` where that's unset.
    sizes: [u32; 3],
    window_tabs: String,
    /// `shadows`: shadows around the HUD's parts (ADR-0113), and how they
    /// look (`shadow_blur`, `shadow_drop` and the rest).
    shadows: bool,
    shadow: ShadowStyle,
    /// The desktop's gaps and the shadows it draws around windows (ADR-0051, ADR-0112), which
    /// are the compositor's config, not the HUD's, and sit on this page with the HUD's own.
    gaps: crate::gaps::GapsSection,
    window_shadows: crate::shadows::ShadowsSection,
    top: bool,
    split: bool,
    /// Each part's spot, in the order of `PARTS`.
    spots: [&'static str; 3],
    /// Each part's `*_mode`, in the order of `PARTS`.
    modes: [&'static str; 3],
    /// A part being dragged on the map, and the spot it would snap to.
    dragging: Option<(usize, usize)>,
}

/// `#rrggbb` as red, green and blue.
fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// The bar config's top-level `key`, as the file says now.
fn read(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(patina_shell::hud::config_path()?).ok()?;
    patina_shell::hud::top_level(&text, key)
}

impl BarPage {
    pub fn new() -> Self {
        let mut page = Self {
            window_tabs: String::new(),
            shadows: true,
            shadow: ShadowStyle::default(),
            gaps: crate::gaps::GapsSection::new(),
            window_shadows: crate::shadows::ShadowsSection::new(),
            top: false,
            split: false,
            spots: [PARTS[0].3, PARTS[1].3, PARTS[2].3],
            modes: ["always"; 3],
            sizes: [HEIGHT; 3],
            dragging: None,
        };
        page.reload();
        page
    }

    /// Reads the settings again, after the file changed.
    pub fn reload(&mut self) {
        self.window_tabs = read("window_tabs")
            .filter(|v| WINDOW_TABS.iter().any(|(value, _)| v == value))
            .unwrap_or_else(|| "both".into());
        self.shadows = read("shadows").is_none_or(|v| v != "false");
        let defaults = ShadowStyle::default();
        let number = |key: &str, default: f32| {
            read(key)
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(default)
        };
        self.shadow = ShadowStyle {
            blur: number("shadow_blur", defaults.blur),
            drop: number("shadow_drop", defaults.drop),
            opacity: number("shadow_opacity", defaults.opacity),
            spread: number("shadow_spread", defaults.spread),
            offset_x: number("shadow_offset_x", defaults.offset_x),
            color: read("shadow_color")
                .and_then(|v| parse_color(&v))
                .unwrap_or(defaults.color),
        };
        self.top = read("position").as_deref() == Some("top");
        // The HUD is the split layout unless the file says "bar".
        self.split = read("layout").as_deref() != Some("bar");
        let height = read("height")
            .and_then(|v| v.parse().ok())
            .unwrap_or(HEIGHT);
        for (i, (key, ..)) in PARTS.iter().enumerate() {
            let own = read(&key.replace("_at", "_height")).and_then(|v| v.parse().ok());
            self.sizes[i] = own.unwrap_or(height);
        }
        for (i, (key, ..)) in PARTS.iter().enumerate() {
            self.modes[i] = read(&key.replace("_at", "_mode"))
                .and_then(|v| MODES.iter().map(|(m, _)| *m).find(|m| *m == v))
                .unwrap_or("always");
        }
        for (i, (key, _, _, default)) in PARTS.iter().enumerate() {
            let value = read(key);
            self.spots[i] = match value.as_deref() {
                // The launcher button can be left out.
                Some("none" | "off" | "hidden") if *key == "launcher_at" => NONE,
                v => v.and_then(spot).unwrap_or(default),
            };
        }
    }

    /// One bar or its parts apart, and where each part goes, with a small
    /// map of the screen.
    fn layout(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Layout").strong());
        for (split, name) in [(false, "One bar"), (true, "Split into its parts")] {
            if ui.radio(self.split == split, name).clicked() && self.split != split {
                self.split = split;
                Self::set("layout", if split { "split" } else { "bar" }, message);
            }
        }
        if !self.split {
            return;
        }
        ui.label(
            RichText::new(
                "The launcher, the window list and the tray each on their own, anywhere along \
                 the screen's edges: a row along the top or bottom, a column up a side. Drag \
                 them on the map, or choose from the lists. A thicker window list is a dock; \
                 a thinner tray is a status line. A part that hides slides off the screen \
                 and comes back when the pointer touches its edge; Super+H hides them all.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            egui::Grid::new("bar-parts").num_columns(4).show(ui, |ui| {
                for (i, (key, name, _, _)) in PARTS.iter().enumerate() {
                    ui.label(*name);
                    let shown = if self.spots[i] == NONE {
                        "Not shown"
                    } else {
                        SPOTS
                            .iter()
                            .find(|(v, ..)| *v == self.spots[i])
                            .map_or(self.spots[i], |(_, label, _)| *label)
                    };
                    egui::ComboBox::from_id_salt(*key)
                        .width(140.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            if *key == "launcher_at"
                                && ui
                                    .selectable_label(self.spots[i] == NONE, "Not shown")
                                    .clicked()
                                && self.spots[i] != NONE
                            {
                                self.spots[i] = NONE;
                                Self::set(key, NONE, message);
                            }
                            for (value, label, _) in SPOTS {
                                if ui.selectable_label(self.spots[i] == value, label).clicked()
                                    && self.spots[i] != value
                                {
                                    self.spots[i] = value;
                                    Self::set(key, value, message);
                                }
                            }
                        });
                    let mode_label = MODES
                        .iter()
                        .find(|(m, _)| *m == self.modes[i])
                        .map_or("", |(_, label)| *label);
                    egui::ComboBox::from_id_salt((*key, "mode"))
                        .width(150.0)
                        .selected_text(mode_label)
                        .show_ui(ui, |ui| {
                            for (value, label) in MODES {
                                if ui.selectable_label(self.modes[i] == value, label).clicked()
                                    && self.modes[i] != value
                                {
                                    self.modes[i] = value;
                                    Self::set(&key.replace("_at", "_mode"), value, message);
                                }
                            }
                        });
                    let size = egui::DragValue::new(&mut self.sizes[i])
                        .range(SIZES)
                        .suffix(" px");
                    let response = ui
                        .add_enabled(self.spots[i] != NONE, size)
                        .on_hover_text("How thick it is across its edge");
                    if response.drag_stopped() || response.lost_focus() {
                        Self::set_number(&key.replace("_at", "_height"), self.sizes[i], message);
                    }
                    ui.end_row();
                }
            });
            ui.add_space(16.0);
            self.map(ui, message);
        });
    }

    /// The screen, small, with each part's mark where it goes. A mark can
    /// be dragged: it snaps to the nearest spot as it moves, and the part
    /// goes there when it's let go.
    fn map(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 160.0), egui::Sense::hover());
        let v = ui.visuals().clone();
        ui.painter().rect_filled(rect, 4.0, v.extreme_bg_color);
        ui.painter().rect_stroke(
            rect,
            4.0,
            v.widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(14.0);
        // Where they can go, while one's being dragged.
        if self.dragging.is_some() {
            for i in 0..SPOTS.len() {
                ui.painter()
                    .circle_filled(spot_point(i, inner), 3.0, v.weak_text_color());
            }
        }
        let mut dropped = None;
        let mut placed: Vec<usize> = Vec::new();
        for (part, (_, name, mark, _)) in PARTS.iter().enumerate() {
            if self.spots[part] == NONE {
                continue;
            }
            let at = match self.dragging {
                Some((p, spot)) if p == part => spot,
                _ => SPOTS
                    .iter()
                    .position(|(v, ..)| *v == self.spots[part])
                    .unwrap_or(0),
            };
            let nth = placed.iter().filter(|&&s| s == at).count();
            placed.push(at);
            let mark_at = mark_rect(at, nth, inner);
            let response = ui
                .interact(
                    mark_at,
                    egui::Id::new(("bar-part", part)),
                    egui::Sense::drag(),
                )
                .on_hover_text(format!("{name}: drag it to where it goes"))
                .on_hover_cursor(egui::CursorIcon::Grab);
            if response.dragged()
                && let Some(pointer) = response.interact_pointer_pos()
            {
                self.dragging = Some((part, nearest(pointer, inner)));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if response.drag_stopped() {
                dropped = self.dragging.take();
            }
            let dragged = matches!(self.dragging, Some((p, _)) if p == part);
            let fill = if dragged {
                v.selection.stroke.color
            } else {
                v.selection.bg_fill
            };
            let ink = if dragged {
                v.selection.bg_fill
            } else {
                v.selection.stroke.color
            };
            ui.painter().rect_filled(mark_at, 3.0, fill);
            ui.painter().text(
                mark_at.center(),
                egui::Align2::CENTER_CENTER,
                *mark,
                egui::TextStyle::Small.resolve(ui.style()),
                ink,
            );
        }
        if let Some((part, at)) = dropped {
            let value = SPOTS[at].0;
            if self.spots[part] != value {
                self.spots[part] = value;
                Self::set(PARTS[part].0, value, message);
            }
        }
    }

    /// Sets `key` to a number, as the bar reads it: a bare integer, which
    /// `patina_shell::hud::set` (strings only) can't write.
    fn set_number(key: &str, value: u32, message: &mut Option<Message>) {
        let result = (|| {
            let path = patina_shell::hud::config_path().ok_or("no home folder")?;
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let text = patina_shell::hud::with_top_level(&text, key, &value.to_string());
            let text = text.replace(&format!("{key} = \"{value}\""), &format!("{key} = {value}"));
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
        })();
        if let Err(e) = result {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    fn set(key: &str, value: &str, message: &mut Option<Message>) {
        if let Err(e) = patina_shell::hud::set(key, value) {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    /// Whether the HUD's parts have shadows, and how they look. Each change
    /// is written when a slider is let go; the HUD follows within a second.
    fn shadows_ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.add_space(4.0);
        if ui
            .checkbox(&mut self.shadows, "Draw shadows around the HUD's parts")
            .changed()
            && let Err(e) = patina_shell::hud::set_bool("shadows", self.shadows)
        {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
        ui.label(
            RichText::new(
                "The desktop draws them, so they also need the shadows around windows, above.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        let failed = std::cell::RefCell::new(None);
        let write = |key: &str, value: f32| {
            if let Err(e) = patina_shell::hud::set_number(key, value as f64) {
                *failed.borrow_mut() = Some(e);
            }
        };
        ui.add_enabled_ui(self.shadows, |ui| {
            egui::Grid::new("hud_shadow").num_columns(2).show(ui, |ui| {
                // The key, its name, the range and what it means.
                let rows: [(&str, &str, &mut f32, std::ops::RangeInclusive<f32>, &str); 5] = [
                    (
                        "shadow_blur",
                        "Size",
                        &mut self.shadow.blur,
                        0.5..=32.0,
                        "How soft the edge is; the shadow reaches about three times this far.",
                    ),
                    (
                        "shadow_opacity",
                        "Opacity",
                        &mut self.shadow.opacity,
                        0.0..=1.0,
                        "How dark it is at the edge of a part.",
                    ),
                    (
                        "shadow_drop",
                        "Drop",
                        &mut self.shadow.drop,
                        -32.0..=32.0,
                        "How far down it is moved; negative moves it up.",
                    ),
                    (
                        "shadow_offset_x",
                        "Sideways",
                        &mut self.shadow.offset_x,
                        -32.0..=32.0,
                        "How far right it is moved; negative moves it left.",
                    ),
                    (
                        "shadow_spread",
                        "Spread",
                        &mut self.shadow.spread,
                        -16.0..=32.0,
                        "How far its shape is grown past the part's before it is blurred.",
                    ),
                ];
                for (key, name, value, range, hint) in rows {
                    ui.label(name).on_hover_text(hint);
                    let slider = ui.add(egui::Slider::new(value, range).fixed_decimals(1));
                    // Saved when it's let go, not on every step of a drag.
                    if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                        write(key, *value);
                    }
                    ui.end_row();
                }
                ui.label("Colour");
                if ui.color_edit_button_srgb(&mut self.shadow.color).changed() {
                    let [r, g, b] = self.shadow.color;
                    if let Err(e) =
                        patina_shell::hud::set("shadow_color", &format!("#{r:02x}{g:02x}{b:02x}"))
                    {
                        *failed.borrow_mut() = Some(e);
                    }
                }
                ui.end_row();
            });
            if ui.button("Reset the shadow to the defaults").clicked() {
                self.shadow = ShadowStyle::default();
                let d = self.shadow;
                for (key, value) in [
                    ("shadow_blur", d.blur),
                    ("shadow_opacity", d.opacity),
                    ("shadow_drop", d.drop),
                    ("shadow_offset_x", d.offset_x),
                    ("shadow_spread", d.spread),
                ] {
                    write(key, value);
                }
                if let Err(e) = patina_shell::hud::set("shadow_color", "#000000") {
                    *failed.borrow_mut() = Some(e);
                }
            }
        });
        if let Some(e) = failed.into_inner() {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("HUD");
        ui.add_space(8.0);

        ui.label(RichText::new("Window Tabs").strong());
        ui.label(
            RichText::new("What each open window's tab on the bar shows.")
                .small()
                .weak(),
        );
        for (value, name) in WINDOW_TABS {
            if ui.radio(self.window_tabs == value, name).clicked() && self.window_tabs != value {
                self.window_tabs = value.to_string();
                Self::set("window_tabs", value, message);
            }
        }
        ui.label(
            RichText::new("Icon-only tabs show the title on hover, and close with a middle click.")
                .small()
                .weak(),
        );
        ui.add_space(16.0);

        self.layout(ui, message);
        ui.add_space(16.0);

        // One bar's edge; split, each part has its own place above.
        if !self.split {
            ui.label(RichText::new("Position").strong());
            for (top, name) in [(false, "Bottom of the screen"), (true, "Top of the screen")] {
                if ui.radio(self.top == top, name).clicked() && self.top != top {
                    self.top = top;
                    Self::set("position", if top { "top" } else { "bottom" }, message);
                }
            }
            ui.add_space(16.0);
        }

        self.gaps.ui(ui, message);
        ui.add_space(16.0);

        // Shadows: the windows' (the compositor's), then the HUD's parts under them.
        self.window_shadows.ui(ui, message);
        self.shadows_ui(ui, message);
        ui.add_space(16.0);

        if let Some(path) = patina_shell::hud::config_path() {
            ui.label(RichText::new("Everything Else").strong());
            ui.label(
                RichText::new(
                    "The modules on each side, the clock's format and the commands the bar runs \
                     are in its config file.",
                )
                .small()
                .weak(),
            );
            if ui
                .button("Open HUD settings")
                .on_hover_text(path.display().to_string())
                .clicked()
            {
                if !path.exists() {
                    Self::set("window_tabs", &self.window_tabs, message);
                }
                if let Err(e) = patina::open::path(&path) {
                    *message = Some(Message::error(format!(
                        "Couldn't open {}: {e}",
                        path.display()
                    )));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0))
    }

    fn index(value: &str) -> usize {
        SPOTS.iter().position(|(v, ..)| *v == value).unwrap()
    }

    #[test]
    fn a_dragged_part_snaps_to_the_nearest_spot() {
        // Near the bottom-left corner, but on the edge: a row there.
        assert_eq!(
            nearest(egui::pos2(35.0, 96.0), inner()),
            index("bottom-left")
        );
        // Up the left side, near its bottom: a column above the corner.
        assert_eq!(
            nearest(egui::pos2(4.0, 70.0), inner()),
            index("left-bottom")
        );
        assert_eq!(nearest(egui::pos2(180.0, 4.0), inner()), index("top-right"));
        assert_eq!(
            nearest(egui::pos2(196.0, 50.0), inner()),
            index("right-center")
        );
    }

    #[test]
    fn rows_lie_along_their_edge_and_columns_up_theirs() {
        let row = mark_rect(index("bottom-left"), 0, inner());
        assert!(row.width() > row.height());
        let column = mark_rect(index("left-bottom"), 0, inner());
        assert!(column.height() > column.width());
        // A second part in the same spot sits beside the first, not on it.
        let second = mark_rect(index("left-bottom"), 1, inner());
        assert!(!second.intersects(column));
    }

    #[test]
    fn old_names_read_as_their_spots() {
        assert_eq!(spot("center-left"), Some("left-center"));
        assert_eq!(spot("bottom"), Some("bottom-center"));
        assert_eq!(spot("nowhere"), None);
    }
}
