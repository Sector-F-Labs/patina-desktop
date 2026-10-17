//! The Bar page (ADR-0050): what the window tabs show, which edge the bar
//! sits on, whether it's one bar or split into its parts and where each
//! goes (experimental), and the bar's config file for everything else. All
//! are top-level keys in the bar's config, which the bar follows within a
//! second.

use egui::{RichText, Ui};

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
    ("windows_at", "Window list", "W", "bottom-center"),
    ("tray_at", "Tray", "T", "bottom-right"),
];

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

pub struct BarPage {
    window_tabs: String,
    top: bool,
    split: bool,
    /// Each part's spot, in the order of `PARTS`.
    spots: [&'static str; 3],
    /// A part being dragged on the map, and the spot it would snap to.
    dragging: Option<(usize, usize)>,
}

/// The bar config's top-level `key`, as the file says now.
fn read(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(patina_shell::bar::config_path()?).ok()?;
    patina_shell::bar::top_level(&text, key)
}

impl BarPage {
    pub fn new() -> Self {
        let mut page = Self {
            window_tabs: String::new(),
            top: false,
            split: false,
            spots: [PARTS[0].3, PARTS[1].3, PARTS[2].3],
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
        self.top = read("position").as_deref() == Some("top");
        self.split = read("layout").as_deref() == Some("split");
        for (i, (key, _, _, default)) in PARTS.iter().enumerate() {
            self.spots[i] = read(key).as_deref().and_then(spot).unwrap_or(default);
        }
    }

    /// One bar or its parts apart, and where each part goes, with a small
    /// map of the screen.
    fn layout(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.label(RichText::new("Layout").strong());
        for (split, name) in [
            (false, "One bar"),
            (true, "Split into its parts (experimental)"),
        ] {
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
                 them on the map, or choose from the lists.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            egui::Grid::new("bar-parts").num_columns(2).show(ui, |ui| {
                for (i, (key, name, _, _)) in PARTS.iter().enumerate() {
                    ui.label(*name);
                    let shown = SPOTS
                        .iter()
                        .find(|(v, ..)| *v == self.spots[i])
                        .map_or(self.spots[i], |(_, label, _)| *label);
                    egui::ComboBox::from_id_salt(*key)
                        .width(140.0)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (value, label, _) in SPOTS {
                                if ui.selectable_label(self.spots[i] == value, label).clicked()
                                    && self.spots[i] != value
                                {
                                    self.spots[i] = value;
                                    Self::set(key, value, message);
                                }
                            }
                        });
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

    fn set(key: &str, value: &str, message: &mut Option<Message>) {
        if let Err(e) = patina_shell::bar::set(key, value) {
            *message = Some(Message::error(format!("Couldn't save the setting: {e}")));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.heading("Bar");
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

        if let Some(path) = patina_shell::bar::config_path() {
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
                .button("Open bar settings")
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
