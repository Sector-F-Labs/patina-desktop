//! The picker's surface: a search box over the emoji in a grid, with the
//! categories along the top, in the active Patina skin. Emoji are drawn as
//! Twemoji images, since egui's own fonts have no colour ones.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{
    Align2, FontId, Key, Modifiers, Rect, Sense, StrokeKind, TextEdit, TextStyle, TextureHandle,
    TextureOptions, Ui, UiBuilder, vec2,
};
use emojis::Emoji;
use patina::skin::Skin;
use patina_shell::Shell;
use twemoji_assets::png::PngTwemojiAsset;

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
    /// Decoded images, made when a cell first shows.
    textures: HashMap<&'static str, Option<TextureHandle>>,
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
            textures: HashMap::new(),
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

    /// The image of `emoji`, decoded the first time.
    fn texture(&mut self, ctx: &egui::Context, emoji: &'static Emoji) -> Option<TextureHandle> {
        self.textures
            .entry(emoji.as_str())
            .or_insert_with(|| {
                let png = PngTwemojiAsset::from_emoji(emoji.as_str())?;
                let image = image::load_from_memory(png).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                Some(ctx.load_texture(emoji.as_str(), image, TextureOptions::LINEAR))
            })
            .clone()
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
        let ctx = ui.ctx().clone();
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
                        self.draw(&ctx, ui, emoji, rect, 20.0);
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
                                self.draw(&ctx, ui, emoji, rect, GLYPH);
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
    /// `emoji` as its image, `size` points, centred in `rect`; as text if it
    /// has none.
    fn draw(&mut self, ctx: &egui::Context, ui: &Ui, emoji: &'static Emoji, rect: Rect, size: f32) {
        let image = Rect::from_center_size(rect.center(), vec2(size, size));
        match self.texture(ctx, emoji) {
            Some(texture) => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(texture.id(), image, uv, egui::Color32::WHITE);
            }
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
