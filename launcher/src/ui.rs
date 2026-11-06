//! The launcher's surface: the list of apps over a search box, in the
//! active Patina skin. Typing filters, Up and Down move the selection,
//! Enter or a click launches, and Escape or clicking elsewhere closes.

use egui::{
    Align2, ColorImage, FontId, Key, Modifiers, Rect, Sense, StrokeKind, TextEdit, TextStyle,
    TextureHandle, TextureOptions, Ui, UiBuilder, vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;

use crate::apps::{self, App};

/// Icon size in points.
pub const ICON: f32 = 28.0;
const ROW_HEIGHT: f32 = 44.0;
const MARGIN: f32 = 12.0;

pub struct Launcher {
    apps: Vec<App>,
    icons: Vec<Option<TextureHandle>>,
    /// Decoded icons waiting to become textures, by app index.
    pub pending: Vec<(usize, ColorImage)>,
    /// Starts the icon thread on the first frame, when the scale is known.
    start_icons: Option<Box<dyn FnOnce(u32)>>,
    query: String,
    /// Index into the current matches.
    selected: usize,
    had_focus: bool,
    done: bool,
    skin: Skin,
}

impl Launcher {
    pub fn new(apps: Vec<App>, skin: Skin, start_icons: Box<dyn FnOnce(u32)>) -> Self {
        Self {
            icons: vec![None; apps.len()],
            apps,
            pending: Vec::new(),
            start_icons: Some(start_icons),
            query: String::new(),
            selected: 0,
            had_focus: false,
            done: false,
            skin,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    fn launch(&mut self, index: usize) {
        let app = &self.apps[index];
        if let Err(e) = apps::launch(app) {
            eprintln!("patina-launcher: can't start {}: {e}", app.name);
        }
        self.done = true;
    }

    fn row(&self, ui: &mut Ui, index: usize, selected: bool) -> egui::Response {
        let app = &self.apps[index];
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let v = ui.visuals();
        let (name_colour, detail_colour) = if selected {
            ui.painter()
                .rect_filled(rect, v.widgets.hovered.corner_radius, v.selection.bg_fill);
            (
                v.selection.stroke.color,
                v.selection.stroke.color.gamma_multiply(0.8),
            )
        } else {
            (v.strong_text_color(), v.weak_text_color())
        };

        let icon = Rect::from_center_size(
            rect.left_center() + vec2(8.0 + ICON / 2.0, 0.0),
            vec2(ICON, ICON),
        );
        match &self.icons[index] {
            Some(texture) => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter()
                    .image(texture.id(), icon, uv, egui::Color32::WHITE);
            }
            None => {
                // Until (or unless) the icon loads: the name's first letter.
                ui.painter().rect_stroke(
                    icon.shrink(2.0),
                    6.0,
                    v.widgets.noninteractive.bg_stroke,
                    StrokeKind::Inside,
                );
                let letter = app
                    .name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                ui.painter().text(
                    icon.center(),
                    Align2::CENTER_CENTER,
                    letter,
                    FontId::proportional(14.0),
                    detail_colour,
                );
            }
        }

        let text_left = icon.right() + 12.0;
        let width = rect.right() - text_left - 8.0;
        let detail = app.generic.as_ref().or(app.comment.as_ref());
        let body = TextStyle::Body.resolve(ui.style());
        let small = TextStyle::Small.resolve(ui.style());
        let name = ui
            .painter()
            .layout(app.name.clone(), body, name_colour, width);
        let lines = match detail {
            Some(d) => {
                let mut job =
                    egui::text::LayoutJob::simple_singleline(d.clone(), small, detail_colour);
                job.wrap = egui::text::TextWrapping::truncate_at_width(width);
                let detail = ui.painter().fonts_mut(|f| f.layout_job(job));
                let top = rect.center().y - (name.size().y + detail.size().y) / 2.0;
                ui.painter().galley(
                    egui::pos2(text_left, top + name.size().y),
                    detail,
                    detail_colour,
                );
                top
            }
            None => rect.center().y - name.size().y / 2.0,
        };
        ui.painter()
            .galley(egui::pos2(text_left, lines), name, name_colour);
        response
    }
}

impl Shell for Launcher {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let ctx = ui.ctx().clone();
        if let Some(start) = self.start_icons.take() {
            start((ICON * ctx.pixels_per_point()).round() as u32);
        }
        for (index, image) in self.pending.drain(..) {
            if let Some(slot) = self.icons.get_mut(index) {
                *slot =
                    Some(ctx.load_texture(format!("icon-{index}"), image, TextureOptions::LINEAR));
            }
        }

        // Clicking another window takes the keyboard away: close, as
        // Spotlight does.
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        let (up, down, enter, escape) = ui.input_mut(|i| {
            (
                // Ctrl+P and Ctrl+N too, as in Emacs, shells and Vim.
                i.consume_key(Modifiers::NONE, Key::ArrowUp)
                    | i.consume_key(Modifiers::CTRL, Key::P),
                i.consume_key(Modifiers::NONE, Key::ArrowDown)
                    | i.consume_key(Modifiers::CTRL, Key::N),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
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
        // Laid out from the bottom: the search box, then the list above it.
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(egui::Layout::bottom_up(egui::Align::Min)),
        );

        let search = ui.add(
            TextEdit::singleline(&mut self.query)
                .hint_text("Search apps")
                .font(TextStyle::Heading)
                .desired_width(f32::INFINITY)
                .margin(vec2(8.0, 6.0)),
        );
        search.request_focus();
        if search.changed() {
            self.selected = 0;
        }
        ui.add_space(8.0);

        let matches = apps::search(&self.apps, &self.query);
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down {
            self.selected += 1;
        }
        self.selected = self.selected.min(matches.len().saturating_sub(1));
        if enter && let Some(&index) = matches.get(self.selected) {
            self.launch(index);
            return;
        }

        // The list reads top down, best match first, in the space left.
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(ui.available_rect_before_wrap())
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        if matches.is_empty() {
            ui.weak("No apps match");
            return;
        }
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (row, &index) in matches.iter().enumerate() {
                    let response = self.row(ui, index, row == self.selected);
                    if response.hovered() && pointer_moved {
                        self.selected = row;
                    }
                    if (up || down) && row == self.selected {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        clicked = Some(index);
                    }
                }
            });
        if let Some(index) = clicked {
            self.launch(index);
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}
