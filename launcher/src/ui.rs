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
use crate::files::Finder;
use patina_search::engine::Hit;

/// Icon size in points.
pub const ICON: f32 = 28.0;
const ROW_HEIGHT: f32 = 44.0;
const MARGIN: f32 = 12.0;

/// Which end of the list the search box sits at.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Search {
    /// Below the list, nearest a bar at the bottom of the screen.
    #[default]
    Bottom,
    /// Above the list, as in Spotlight.
    Top,
}

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
    search: Search,
    /// The files found as you type, if the launcher searches any.
    finder: Option<Finder>,
}

/// What a row of the list is.
#[derive(Clone, Copy)]
enum Entry {
    App(usize),
    File(usize),
}

impl Launcher {
    pub fn new(
        apps: Vec<App>,
        skin: Skin,
        search: Search,
        finder: Option<Finder>,
        start_icons: Box<dyn FnOnce(u32)>,
    ) -> Self {
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
            search,
            finder,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    /// The files found for question `number`.
    pub fn files_found(&mut self, number: u64, hits: Vec<Hit>) {
        if let Some(finder) = &mut self.finder {
            finder.found(number, hits);
        }
    }

    fn open(&mut self, entry: Entry, files: &[Hit]) {
        match entry {
            Entry::App(index) => self.launch(index),
            Entry::File(index) => {
                if let Err(e) = patina::open::path(&files[index].path) {
                    eprintln!(
                        "patina-launcher: can't open {}: {e}",
                        files[index].path.display()
                    );
                }
                self.done = true;
            }
        }
    }

    fn launch(&mut self, index: usize) {
        let app = &self.apps[index];
        if let Err(e) = apps::launch(app) {
            eprintln!("patina-launcher: can't start {}: {e}", app.name);
        }
        self.done = true;
    }

    /// A row's space, lit if it's the selected one, and the colours its name
    /// and detail are in.
    fn row_frame(
        &self,
        ui: &mut Ui,
        selected: bool,
    ) -> (Rect, egui::Response, egui::Color32, egui::Color32) {
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
        (rect, response, name_colour, detail_colour)
    }

    /// The icon's place in a row.
    fn icon_rect(rect: Rect) -> Rect {
        Rect::from_center_size(
            rect.left_center() + vec2(8.0 + ICON / 2.0, 0.0),
            vec2(ICON, ICON),
        )
    }

    /// A name, and a line of detail under it, in a row.
    fn row_text(
        ui: &Ui,
        rect: Rect,
        icon: Rect,
        name: &str,
        detail: Option<&str>,
        colours: (egui::Color32, egui::Color32),
    ) {
        let (name_colour, detail_colour) = colours;
        let text_left = icon.right() + 12.0;
        let width = rect.right() - text_left - 8.0;
        let body = TextStyle::Body.resolve(ui.style());
        let small = TextStyle::Small.resolve(ui.style());
        let name = ui
            .painter()
            .layout(name.to_string(), body, name_colour, width);
        let top = match detail {
            Some(d) => {
                let mut job =
                    egui::text::LayoutJob::simple_singleline(d.to_string(), small, detail_colour);
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
            .galley(egui::pos2(text_left, top), name, name_colour);
    }

    fn row(&self, ui: &mut Ui, index: usize, selected: bool) -> egui::Response {
        let app = &self.apps[index];
        let (rect, response, name_colour, detail_colour) = self.row_frame(ui, selected);
        let v = ui.visuals();
        let icon = Self::icon_rect(rect);
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
        let detail = app.generic.as_ref().or(app.comment.as_ref());
        Self::row_text(
            ui,
            rect,
            icon,
            &app.name,
            detail.map(String::as_str),
            (name_colour, detail_colour),
        );
        response
    }

    /// A file found: a page for its icon, its name, and where it is, or the
    /// line it matched on.
    fn file_row(&self, ui: &mut Ui, hit: &Hit, selected: bool) -> egui::Response {
        let (rect, response, name_colour, detail_colour) = self.row_frame(ui, selected);
        let icon = Self::icon_rect(rect);
        // A sheet with its corner turned down.
        let page = Rect::from_center_size(icon.center(), vec2(ICON * 0.62, ICON * 0.8));
        let fold = page.width() * 0.32;
        let stroke = egui::Stroke::new(1.5, detail_colour);
        ui.painter().add(egui::Shape::closed_line(
            vec![
                page.left_top(),
                page.right_top() - vec2(fold, 0.0),
                page.right_top() + vec2(0.0, fold),
                page.right_bottom(),
                page.left_bottom(),
            ],
            stroke,
        ));
        ui.painter().add(egui::Shape::line(
            vec![
                page.right_top() - vec2(fold, 0.0),
                page.right_top() + vec2(-fold, fold),
                page.right_top() + vec2(0.0, fold),
            ],
            stroke,
        ));
        let detail = match &hit.line {
            Some((n, text)) => format!("{n}: {text}"),
            None => hit.dir.clone(),
        };
        Self::row_text(
            ui,
            rect,
            icon,
            &hit.name,
            Some(&detail),
            (name_colour, detail_colour),
        );
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
            patina_shell::hud::corner_radius(),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        // Laid out from the search box's end: the box, then the list
        // beyond it.
        let layout = match self.search {
            Search::Bottom => egui::Layout::bottom_up(egui::Align::Min),
            Search::Top => egui::Layout::top_down(egui::Align::Min),
        };
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(layout),
        );

        let files_too = self.finder.as_ref().is_some_and(Finder::usable);
        let hint = if files_too {
            "Search apps and files"
        } else {
            "Search apps"
        };
        let search = ui.add(
            TextEdit::singleline(&mut self.query)
                .hint_text(egui::RichText::new(hint).text_style(TextStyle::Heading))
                .font(TextStyle::Heading)
                .desired_width(f32::INFINITY)
                .margin(vec2(8.0, 6.0)),
        );
        search.request_focus();
        if search.changed() {
            self.selected = 0;
            if let Some(finder) = &mut self.finder {
                finder.ask(&self.query);
            }
        }
        ui.add_space(8.0);

        let apps_found = apps::search(&self.apps, &self.query);
        let files: Vec<Hit> = self
            .finder
            .as_ref()
            .map_or_else(Vec::new, |f| f.hits.clone());
        // The apps, then the files.
        let entries: Vec<Entry> = apps_found
            .iter()
            .map(|&i| Entry::App(i))
            .chain((0..files.len()).map(Entry::File))
            .collect();
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down {
            self.selected += 1;
        }
        self.selected = self.selected.min(entries.len().saturating_sub(1));
        if enter && let Some(&entry) = entries.get(self.selected) {
            self.open(entry, &files);
            return;
        }

        // The list reads top down, best match first, in the space left.
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(ui.available_rect_before_wrap())
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        if entries.is_empty() {
            ui.weak(if files_too {
                "No matches"
            } else {
                "No apps match"
            });
            return;
        }
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let mut clicked = None;
        // With both, each kind has a small heading.
        let headings = !apps_found.is_empty() && !files.is_empty();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (row, &entry) in entries.iter().enumerate() {
                    if headings && (row == 0 || row == apps_found.len()) {
                        let heading = if row == 0 { "Apps" } else { "Files" };
                        ui.label(egui::RichText::new(heading).small().weak());
                    }
                    let response = match entry {
                        Entry::App(index) => self.row(ui, index, row == self.selected),
                        Entry::File(index) => {
                            self.file_row(ui, &files[index], row == self.selected)
                        }
                    };
                    if response.hovered() && pointer_moved {
                        self.selected = row;
                    }
                    if (up || down) && row == self.selected {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        clicked = Some(entry);
                    }
                }
            });
        if let Some(entry) = clicked {
            self.open(entry, &files);
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}
