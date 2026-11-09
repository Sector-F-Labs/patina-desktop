//! The picker's surface: a search box over the emoji in a grid, with the
//! categories along the top, in the active Patina skin. Emoji are drawn as
//! images from Patina's emoji (Noto Color Emoji, shared with Teleprinter), since
//! egui's own fonts have no colour ones.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{
    Align2, FontId, Key, Modifiers, Rect, Sense, StrokeKind, TextEdit, TextStyle, Ui, UiBuilder,
    vec2,
};
use emojis::Emoji;
use patina::skin::Skin;
use patina_shell::Shell;

use crate::data::{self, GROUPS};

const COLUMNS: usize = 8;
const CELL: f32 = 40.0;
const GLYPH: f32 = 28.0;
const MARGIN: f32 = 12.0;

/// What's shown under the search box when it's empty.
#[derive(Clone, Copy, PartialEq)]
enum Category {
    Recent,
    Group(usize),
}

pub struct Picker {
    skin: Skin,
    recent: Vec<&'static Emoji>,
    groups: Vec<Vec<&'static Emoji>>,
    category: Category,
    query: String,
    selected: usize,
    /// Each emoji's glyph in Patina's emoji font, looked up when it first shows.
    glyphs: HashMap<&'static str, Option<patina::emoji::Glyph>>,
    had_focus: bool,
    done: bool,
    picked: Arc<Mutex<Option<String>>>,
}

impl Picker {
    pub fn new(
        skin: Skin,
        recent: Vec<&'static Emoji>,
        picked: Arc<Mutex<Option<String>>>,
    ) -> Self {
        let category = if recent.is_empty() {
            Category::Group(0)
        } else {
            Category::Recent
        };
        Self {
            skin,
            recent,
            groups: GROUPS.iter().map(|(g, _)| data::group(*g)).collect(),
            category,
            query: String::new(),
            selected: 0,
            glyphs: HashMap::new(),
            had_focus: false,
            done: false,
            picked,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    fn pick(&mut self, emoji: &Emoji) {
        if let Ok(mut picked) = self.picked.lock() {
            *picked = Some(emoji.as_str().to_string());
        }
        self.done = true;
    }

    fn categories(&self) -> Vec<Category> {
        let mut all = Vec::new();
        if !self.recent.is_empty() {
            all.push(Category::Recent);
        }
        all.extend((0..GROUPS.len()).map(Category::Group));
        all
    }

    fn shown(&self) -> Vec<&'static Emoji> {
        if !self.query.trim().is_empty() {
            return data::search(&self.query);
        }
        match self.category {
            Category::Recent => self.recent.clone(),
            Category::Group(i) => self.groups[i].clone(),
        }
    }

    fn category_name(&self, category: Category) -> &'static str {
        match category {
            Category::Recent => "Recent",
            Category::Group(i) => GROUPS[i].1,
        }
    }

    /// The emoji a category is shown by.
    fn category_emoji(&self, category: Category) -> Option<&'static Emoji> {
        match category {
            Category::Recent => emojis::get("🕘"),
            Category::Group(i) => {
                emojis::get(["😀", "👋", "🐻", "🍔", "🚗", "⚽", "💡", "💯", "🏁"][i])
            }
        }
    }
}

impl Shell for Picker {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        // Clicking another window takes the keyboard away: close.
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        let (left, right, up, down, enter, escape, tab, back_tab) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.consume_key(Modifiers::NONE, Key::ArrowRight),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Tab),
                i.consume_key(Modifiers::SHIFT, Key::Tab),
            )
        });
        if escape {
            self.done = true;
            return;
        }

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            v.menu_corner_radius,
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        let search = ui.add(
            TextEdit::singleline(&mut self.query)
                .hint_text("Search emoji")
                .font(TextStyle::Heading)
                .desired_width(f32::INFINITY)
                .margin(vec2(8.0, 6.0)),
        );
        search.request_focus();
        if search.changed() {
            self.selected = 0;
        }
        ui.add_space(6.0);

        // The categories, while not searching.
        let searching = !self.query.trim().is_empty();
        if !searching {
            let all = self.categories();
            if tab || back_tab {
                let at = all.iter().position(|c| *c == self.category).unwrap_or(0);
                let next = if back_tab {
                    (at + all.len() - 1) % all.len()
                } else {
                    (at + 1) % all.len()
                };
                self.category = all[next];
                self.selected = 0;
            }
            let mut chosen = None;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for category in all {
                    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 30.0), Sense::click());
                    if category == self.category {
                        ui.painter().rect_filled(
                            rect,
                            v.widgets.hovered.corner_radius,
                            v.selection.bg_fill,
                        );
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            v.widgets.hovered.corner_radius,
                            v.widgets.hovered.weak_bg_fill,
                        );
                    }
                    if let Some(emoji) = self.category_emoji(category) {
                        self.draw(ui, emoji, rect, 20.0);
                    }
                    if response
                        .on_hover_text(self.category_name(category))
                        .clicked()
                    {
                        chosen = Some(category);
                    }
                }
            });
            if let Some(category) = chosen {
                self.category = category;
                self.selected = 0;
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(self.category_name(self.category))
                    .small()
                    .weak(),
            );
        } else {
            ui.add_space(4.0);
        }

        let shown = self.shown();
        if shown.is_empty() {
            ui.add_space(8.0);
            ui.weak("No emoji match");
            return;
        }
        let moved = left || right || up || down;
        if left {
            self.selected = self.selected.saturating_sub(1);
        }
        if right {
            self.selected += 1;
        }
        if up {
            self.selected = self.selected.saturating_sub(COLUMNS);
        }
        if down {
            self.selected += COLUMNS;
        }
        self.selected = self.selected.min(shown.len() - 1);
        if enter {
            self.pick(shown[self.selected]);
            return;
        }

        // The selected one's name, under the grid.
        let name_height = 22.0;
        let grid = ui.available_rect_before_wrap();
        let grid = Rect::from_min_max(grid.min, grid.max - vec2(0.0, name_height));
        ui.painter().text(
            egui::pos2(grid.left() + 4.0, grid.bottom() + name_height / 2.0 + 2.0),
            Align2::LEFT_CENTER,
            shown[self.selected].name(),
            TextStyle::Body.resolve(ui.style()),
            v.weak_text_color(),
        );
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let mut clicked = None;
        let mut grid_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(grid)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut grid_ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for (row, chunk) in shown.chunks(COLUMNS).enumerate() {
                    ui.horizontal(|ui| {
                        for (column, emoji) in chunk.iter().enumerate() {
                            let index = row * COLUMNS + column;
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(CELL, CELL), Sense::click());
                            if response.hovered() && pointer_moved {
                                self.selected = index;
                            }
                            if index == self.selected {
                                ui.painter().rect_filled(
                                    rect.shrink(1.0),
                                    v.widgets.hovered.corner_radius,
                                    v.selection.bg_fill,
                                );
                                if moved {
                                    response.scroll_to_me(None);
                                }
                            }
                            if ui.is_rect_visible(rect) {
                                self.draw(ui, emoji, rect, GLYPH);
                            }
                            if response.clicked() {
                                clicked = Some(*emoji);
                            }
                        }
                    });
                }
            });
        if let Some(emoji) = clicked {
            self.pick(emoji);
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

impl Picker {
    /// `emoji` as its image (Patina's, as Teleprinter's), `size` points,
    /// centred in `rect`; as text if it has none.
    fn draw(&mut self, ui: &Ui, emoji: &'static Emoji, rect: Rect, size: f32) {
        let image = Rect::from_center_size(rect.center(), vec2(size, size));
        let glyph = *self
            .glyphs
            .entry(emoji.as_str())
            .or_insert_with(|| patina::emoji::glyph(emoji.as_str()));
        match glyph {
            Some(glyph) => patina::emoji::paint(ui.painter(), glyph, image),
            None => {
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    emoji.as_str(),
                    FontId::proportional(size * 0.8),
                    ui.visuals().text_color(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picker in each built-in skin, as PNGs in the folder PATINA_EMOJI_PREVIEW
    /// names, so how it looks can be seen without a desktop: first as it opens, with
    /// the recent ones kept, then searching, then on another category:
    /// `PATINA_EMOJI_PREVIEW=/tmp/p cargo test -p patina-emoji preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_picker() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_EMOJI_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        patina::emoji::install(patina_emoji_font::FONT);
        let scale = 2.0;
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let recent: Vec<&'static Emoji> =
            ["👍", "😂", "🎉", "❤️", "🙏", "🔥", "✅", "🤔", "👀", "🚀"]
                .iter()
                .filter_map(|e| emojis::get(e))
                .collect();
        for skin in skins {
            let name = skin.name.clone();
            for state in ["recent", "search", "food"] {
                let mut picker =
                    Picker::new(skin.clone(), recent.clone(), Arc::new(Mutex::new(None)));
                match state {
                    "search" => picker.query = "heart".into(),
                    "food" => picker.category = Category::Group(3),
                    _ => {}
                }
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(scale);
                picker.setup(&ctx);
                let (w, h) = (
                    (SIZE_POINTS.0 * scale) as usize,
                    (SIZE_POINTS.1 * scale) as usize,
                );
                let mut painter = patina_shell::raster::Painter::default();
                let mut pixels = vec![0u32; w * h];
                // A few passes: the search box takes focus, the images arrive.
                for _ in 0..4 {
                    let input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::pos2(0.0, 0.0),
                            vec2(SIZE_POINTS.0, SIZE_POINTS.1),
                        )),
                        max_texture_side: Some(8192),
                        focused: true,
                        ..Default::default()
                    };
                    let out = ctx.run_ui(input, |ui| picker.ui(ui, ""));
                    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                    painter.paint(
                        &mut pixels,
                        w,
                        h,
                        out.pixels_per_point,
                        &primitives,
                        &out.textures_delta,
                    );
                }
                let rgba: Vec<u8> = pixels
                    .iter()
                    .flat_map(|p| {
                        let [b, g, r, a] = p.to_le_bytes();
                        [r, g, b, a]
                    })
                    .collect();
                let file = dir.join(format!("emoji-{name}-{state}.png"));
                image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .unwrap();
            }
        }
    }

    /// The picker's size in points, as `main.rs` opens it.
    const SIZE_POINTS: (f32, f32) = (400.0, 440.0);
}
