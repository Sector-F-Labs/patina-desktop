//! The spreadsheet grid: painting, selection, keyboard and editing.
//!
//! Only the cells in view are painted, so a sheet can be as long as it likes.
//! Colours come from the active `Visuals`, so skins reach the grid.

use egui::{Align2, Event, Key, Modifiers, Pos2, Rangef, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};

use crate::formula::{Pos, Value, cell_name, col_name};
use crate::sheet::Sheet;

pub const COL_WIDTH: f32 = 112.0;
const HEADER_WIDTH: f32 = 48.0;
const PAD: f32 = 6.0;
/// The most rows and columns a sheet has (Excel's).
const MAX_ROWS: u32 = 1_048_576;
const MAX_COLS: u32 = 16_384;

/// A cell edited in place.
struct Edit {
    text: String,
    /// Frames drawn so far; focus is only checked after the first.
    frames: u32,
    /// Put the caret at the end (typing started the edit) rather than keeping
    /// it where egui puts it.
    caret_at_end: bool,
}

/// What the grid remembers about one sheet between frames.
#[derive(Default)]
pub struct View {
    /// The cell the keyboard acts on.
    pub cursor: Pos,
    /// The other corner of the selection; equal to `cursor` for one cell.
    pub anchor: Pos,
    edit: Option<Edit>,
    /// Scroll the cursor into view on the next frame.
    reveal: bool,
}

impl View {
    /// The selection's corners, top-left first.
    pub fn selection(&self) -> (Pos, Pos) {
        (
            (self.cursor.0.min(self.anchor.0), self.cursor.1.min(self.anchor.1)),
            (self.cursor.0.max(self.anchor.0), self.cursor.1.max(self.anchor.1)),
        )
    }

    /// Starts editing the cursor's cell with what it holds.
    pub fn begin_edit(&mut self, sheet: &Sheet) {
        self.edit = Some(Edit { text: sheet.input(self.cursor).to_string(), frames: 0, caret_at_end: true });
    }

    fn move_to(&mut self, pos: Pos, extend: bool) {
        self.cursor = (pos.0.min(MAX_ROWS - 1), pos.1.min(MAX_COLS - 1));
        if !extend {
            self.anchor = self.cursor;
        }
        self.reveal = true;
    }

    fn step(&mut self, dr: i64, dc: i64, extend: bool) {
        let r = (self.cursor.0 as i64 + dr).clamp(0, MAX_ROWS as i64 - 1) as u32;
        let c = (self.cursor.1 as i64 + dc).clamp(0, MAX_COLS as i64 - 1) as u32;
        self.move_to((r, c), extend);
    }
}

/// The cells set by this frame, for the file.
pub type Changes = Vec<(Pos, String)>;

fn set(sheet: &mut Sheet, pos: Pos, input: &str, changes: &mut Changes) {
    if sheet.set(pos, input) {
        changes.push((pos, input.to_string()));
    }
}

/// Rows and columns to show: the data, the cursor, and room beyond them.
fn dimensions(sheet: &Sheet, view: &View) -> (u32, u32) {
    let (er, ec) = sheet.extent();
    let rows = (er.max(view.cursor.0 + 1) + 40).clamp(100, MAX_ROWS);
    let cols = (ec.max(view.cursor.1 + 1) + 6).clamp(26, MAX_COLS);
    (rows, cols)
}

/// Text for the clipboard: the selection's values, tab and newline separated.
fn copy_text(sheet: &Sheet, (r0, c0): Pos, (r1, c1): Pos) -> String {
    (r0..=r1)
        .map(|r| (c0..=c1).map(|c| sheet.value((r, c)).display()).collect::<Vec<_>>().join("\t"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pastes tab/newline separated text with its top-left at `at`; returns the
/// block's size.
fn paste(sheet: &mut Sheet, at: Pos, text: &str, changes: &mut Changes) -> Pos {
    let text = text.replace("\r\n", "\n");
    let text = text.strip_suffix('\n').unwrap_or(&text);
    let (mut rows, mut cols) = (0, 0);
    for (i, line) in text.split('\n').enumerate() {
        for (j, cell) in line.split('\t').enumerate() {
            let pos = (at.0 + i as u32, at.1 + j as u32);
            if pos.0 < MAX_ROWS && pos.1 < MAX_COLS {
                set(sheet, pos, cell, changes);
            }
            cols = cols.max(j as u32 + 1);
        }
        rows = i as u32 + 1;
    }
    (rows.max(1), cols.max(1))
}

fn clear(sheet: &mut Sheet, (r0, c0): Pos, (r1, c1): Pos, changes: &mut Changes) {
    for r in r0..=r1 {
        for c in c0..=c1 {
            set(sheet, (r, c), "", changes);
        }
    }
}

/// Keeps what's being typed, as when another tab is shown.
pub fn finish(sheet: &mut Sheet, view: &mut View) -> Changes {
    let mut changes = Vec::new();
    if let Some(edit) = view.edit.take() {
        set(sheet, view.cursor, &edit.text, &mut changes);
    }
    changes
}

/// Shows the grid and applies the user's edits to `sheet`, returning them for
/// the file. `keys` is false while a dialog or another widget owns the keyboard.
pub fn show(ui: &mut Ui, sheet: &mut Sheet, view: &mut View, keys: bool) -> Changes {
    let mut changes = Vec::new();
    let ctx = ui.ctx().clone();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let row_h = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
    let (rows, cols) = dimensions(sheet, view);
    let visuals = ui.visuals().clone();
    let (sel_a, sel_b) = view.selection();

    // Keyboard, before the widgets that might want the keys.
    let mut finish: Option<(i64, i64)> = None; // commit the edit, then move
    let mut cancel = false;
    if keys {
        if view.edit.is_some() {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                finish = Some((1, 0));
            } else if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter)) {
                finish = Some((-1, 0));
            } else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Tab)) {
                finish = Some((0, 1));
            } else if ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Tab)) {
                finish = Some((0, -1));
            } else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                cancel = true;
            }
        } else {
            handle_keys(&ctx, sheet, view, &mut changes);
        }
    }
    if cancel {
        view.edit = None;
    }
    if let Some((dr, dc)) = finish {
        if let Some(edit) = view.edit.take() {
            set(sheet, view.cursor, &edit.text, &mut changes);
        }
        view.step(dr, dc, false);
    }

    egui::ScrollArea::both().auto_shrink(false).show_viewport(ui, |ui, viewport| {
        let size = Vec2::new(HEADER_WIDTH + cols as f32 * COL_WIDTH, row_h + rows as f32 * row_h);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let origin = rect.min;
        let cell_rect = |(r, c): Pos| {
            Rect::from_min_size(
                origin + Vec2::new(HEADER_WIDTH + c as f32 * COL_WIDTH, row_h + r as f32 * row_h),
                Vec2::new(COL_WIDTH, row_h),
            )
        };
        let cell_at = |p: Pos2| -> Option<Pos> {
            let rel = p - origin;
            if rel.x < HEADER_WIDTH || rel.y < row_h {
                return None;
            }
            let c = ((rel.x - HEADER_WIDTH) / COL_WIDTH) as u32;
            let r = ((rel.y - row_h) / row_h) as u32;
            (r < rows && c < cols).then_some((r, c))
        };
        // What's visible below and right of the sticky headers.
        let first_row = (((viewport.min.y) / row_h).floor().max(1.0) as u32 - 1).min(rows - 1);
        let last_row = ((viewport.max.y / row_h).ceil() as u32).min(rows - 1);
        let first_col = ((viewport.min.x / COL_WIDTH).floor().max(0.0) as u32).min(cols - 1);
        let last_col = (((viewport.max.x) / COL_WIDTH).ceil() as u32).min(cols - 1);

        // Pointer: press selects, drag extends, double-click edits.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let sticky = |p: Pos2| p.x - rect.min.x < viewport.min.x + HEADER_WIDTH || p.y - rect.min.y < viewport.min.y + row_h;
        if let Some(p) = pointer.filter(|p| response.contains_pointer() && !sticky(*p)) {
            let pressed = ui.input(|i| i.pointer.primary_pressed());
            let shift = ui.input(|i| i.modifiers.shift);
            if pressed && let Some(pos) = cell_at(p) {
                if let Some(edit) = view.edit.take() {
                    set(sheet, view.cursor, &edit.text, &mut changes);
                }
                view.move_to(pos, shift);
                view.reveal = false;
                if keys {
                    ui.memory_mut(|m| m.surrender_focus(egui::Id::new("grid_editor")));
                }
            }
            if response.dragged()
                && view.edit.is_none()
                && let Some(pos) = cell_at(p)
                && pos != view.cursor
            {
                view.cursor = pos;
            }
            if response.double_clicked() && view.edit.is_none() {
                view.begin_edit(sheet);
            }
        }

        let painter = ui.painter_at(ui.clip_rect());
        let grid_line = Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color.gamma_multiply(0.6));

        // Cells.
        for r in first_row..=last_row {
            for c in first_col..=last_col {
                let pos = (r, c);
                let cr = cell_rect(pos);
                let selected = (sel_a.0..=sel_b.0).contains(&r) && (sel_a.1..=sel_b.1).contains(&c);
                if selected {
                    painter.rect_filled(cr, 0.0, visuals.selection.bg_fill.gamma_multiply(0.35));
                }
                if sheet.block_at(pos).is_some() {
                    // Filled by a shell command: not typed, so not editable in place.
                    painter.rect_filled(cr, 0.0, visuals.selection.bg_fill.gamma_multiply(0.10));
                }
                painter.rect_stroke(cr, 0.0, grid_line, StrokeKind::Inside);
                if view.edit.is_some() && pos == view.cursor {
                    continue;
                }
                let value = sheet.value(pos);
                if value == Value::Empty {
                    continue;
                }
                let color = match &value {
                    Value::Error(_) => visuals.error_fg_color,
                    _ => visuals.text_color(),
                };
                let clipped = painter.with_clip_rect(cr.shrink2(Vec2::new(2.0, 0.0)));
                let (anchor, at) = match value {
                    Value::Number(_) => (Align2::RIGHT_CENTER, cr.right_center() - Vec2::new(PAD, 0.0)),
                    _ => (Align2::LEFT_CENTER, cr.left_center() + Vec2::new(PAD, 0.0)),
                };
                let line = patina::emoji::line(&clipped, &value.display(), font.clone(), color, f32::INFINITY);
                let x = if anchor == Align2::RIGHT_CENTER { at.x - line.size().x } else { at.x };
                line.paint(&clipped, Pos2::new(x, at.y - line.size().y / 2.0));
            }
        }

        // Shell output that didn't fit its block: a heavier edge with a "…" on it.
        let warn = visuals.warn_fg_color;
        let seen_x = Rangef::new(origin.x + viewport.min.x + HEADER_WIDTH, origin.x + viewport.max.x);
        let seen_y = Rangef::new(origin.y + viewport.min.y + row_h, origin.y + viewport.max.y);
        for b in sheet.blocks().iter().filter(|b| b.cut_lines > 0 || b.cut_fields > 0) {
            let block = Rect::from_min_max(cell_rect(b.owner).min, cell_rect(b.end()).max);
            let chip = |center: Pos2, id: &str, text: String| {
                let rect = Rect::from_center_size(center, Vec2::new(26.0, 16.0));
                painter.rect_filled(rect, 4.0, visuals.panel_fill);
                painter.rect_stroke(rect, 4.0, Stroke::new(1.5, warn), StrokeKind::Inside);
                painter.text(rect.center() - Vec2::new(0.0, 3.0), Align2::CENTER_CENTER, "…", font.clone(), warn);
                ui.interact(rect, egui::Id::new((id, b.owner)), Sense::hover()).on_hover_text(text);
            };
            let shown_x = Rangef::new(block.left().max(seen_x.min), block.right().min(seen_x.max));
            let shown_y = Rangef::new(block.top().max(seen_y.min), block.bottom().min(seen_y.max));
            if b.cut_lines > 0 && seen_y.contains(block.bottom()) && shown_x.min < shown_x.max {
                painter.line_segment([Pos2::new(block.left(), block.bottom()), Pos2::new(block.right(), block.bottom())], Stroke::new(3.0, warn));
                let s = if b.cut_lines == 1 { "" } else { "s" };
                chip(
                    Pos2::new(shown_x.center(), block.bottom()),
                    "cut-lines",
                    format!(
                        "{} more line{s} not shown. The command printed {}; this block holds {}.\nExtend the range, or leave its end row off.",
                        b.cut_lines,
                        b.rows + b.cut_lines,
                        b.rows
                    ),
                );
            }
            if b.cut_fields > 0 && seen_x.contains(block.right()) && shown_y.min < shown_y.max {
                painter.line_segment([Pos2::new(block.right(), block.top()), Pos2::new(block.right(), block.bottom())], Stroke::new(3.0, warn));
                let s = if b.cut_fields == 1 { "" } else { "s" };
                chip(
                    Pos2::new(block.right(), shown_y.center()),
                    "cut-fields",
                    format!(
                        "Up to {} more field{s} per line not shown (rest=drop).\nExtend the range, or use rest=keep to keep them in the last column.",
                        b.cut_fields
                    ),
                );
            }
        }

        // The cursor cell's outline, and its editor.
        let cursor_rect = cell_rect(view.cursor);
        painter.rect_stroke(cursor_rect, 0.0, Stroke::new(2.0, visuals.selection.stroke.color), StrokeKind::Inside);
        if view.reveal {
            ui.scroll_to_rect(cursor_rect.expand2(Vec2::new(HEADER_WIDTH, row_h)), None);
            view.reveal = false;
        }
        if let Some(edit) = view.edit.as_mut() {
            let id = egui::Id::new("grid_editor");
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(cursor_rect));
            let out = TextEdit::singleline(&mut edit.text)
                .id(id)
                .frame(egui::Frame::NONE)
                .margin(Vec2::new(PAD, 0.0))
                .vertical_align(egui::Align::Center)
                .desired_width(cursor_rect.width())
                .min_size(cursor_rect.size())
                .show(&mut ui);
            if edit.caret_at_end && edit.frames == 0 {
                let mut state = out.state.clone();
                let end = egui::text::CCursor::new(edit.text.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(&ctx, id);
            }
            // The click that opened the editor counts as a click elsewhere to egui, which
            // takes focus away as the editor draws; ask again once it has.
            if edit.frames < 2 {
                out.response.request_focus();
            }
            edit.frames += 1;
            // Focus gone to somewhere else (a menu, another window): keep what was typed.
            if edit.frames > 2 && !out.response.has_focus() {
                let text = std::mem::take(&mut edit.text);
                view.edit = None;
                set(sheet, view.cursor, &text, &mut changes);
            }
        }

        // Sticky headers, painted over the cells.
        let top = origin.y + viewport.min.y;
        let left = origin.x + viewport.min.x;
        let header_fill = visuals.widgets.noninteractive.weak_bg_fill;
        let header_ink = visuals.weak_text_color();
        let hot = visuals.selection.bg_fill.gamma_multiply(0.45);
        let header = |rect: Rect, label: String, active: bool| {
            painter.rect_filled(rect, 0.0, header_fill);
            if active {
                painter.rect_filled(rect, 0.0, hot);
            }
            painter.rect_stroke(rect, 0.0, grid_line, StrokeKind::Inside);
            let ink = if active { visuals.strong_text_color() } else { header_ink };
            painter.text(rect.center(), Align2::CENTER_CENTER, label, font.clone(), ink);
        };
        for c in first_col..=last_col {
            let r = Rect::from_min_size(
                Pos2::new(origin.x + HEADER_WIDTH + c as f32 * COL_WIDTH, top),
                Vec2::new(COL_WIDTH, row_h),
            );
            header(r, col_name(c), (sel_a.1..=sel_b.1).contains(&c));
        }
        for r in first_row..=last_row {
            let rr = Rect::from_min_size(
                Pos2::new(left, origin.y + row_h + r as f32 * row_h),
                Vec2::new(HEADER_WIDTH, row_h),
            );
            header(rr, (r + 1).to_string(), (sel_a.0..=sel_b.0).contains(&r));
        }
        header(Rect::from_min_size(Pos2::new(left, top), Vec2::new(HEADER_WIDTH, row_h)), String::new(), false);
    });
    changes
}

/// Keys with no edit in progress: movement, clearing, clipboard, starting an edit.
fn handle_keys(ctx: &egui::Context, sheet: &mut Sheet, view: &mut View, changes: &mut Changes) {
    let events = ctx.input(|i| i.events.clone());
    let page = 20;
    for event in events {
        match event {
            Event::Key { key, pressed: true, modifiers, .. } => {
                let extend = modifiers.shift;
                let jump = modifiers.command;
                match key {
                    Key::ArrowDown if jump => view.move_to((jump_to(sheet, view.cursor, 1, 0), view.cursor.1), extend),
                    Key::ArrowUp if jump => view.move_to((jump_to(sheet, view.cursor, -1, 0), view.cursor.1), extend),
                    Key::ArrowRight if jump => view.move_to((view.cursor.0, jump_to(sheet, view.cursor, 0, 1)), extend),
                    Key::ArrowLeft if jump => view.move_to((view.cursor.0, jump_to(sheet, view.cursor, 0, -1)), extend),
                    Key::ArrowDown => view.step(1, 0, extend),
                    Key::ArrowUp => view.step(-1, 0, extend),
                    Key::ArrowRight => view.step(0, 1, extend),
                    Key::ArrowLeft => view.step(0, -1, extend),
                    Key::PageDown if !modifiers.command => view.step(page, 0, extend),
                    Key::PageUp if !modifiers.command => view.step(-page, 0, extend),
                    Key::Home if jump => view.move_to((0, 0), extend),
                    Key::Home => view.move_to((view.cursor.0, 0), extend),
                    Key::Enter if modifiers.shift => view.step(-1, 0, false),
                    Key::Enter => view.step(1, 0, false),
                    Key::Tab if modifiers.shift => view.step(0, -1, false),
                    Key::Tab => view.step(0, 1, false),
                    Key::F2 => view.begin_edit(sheet),
                    Key::Delete | Key::Backspace => {
                        let (a, b) = view.selection();
                        clear(sheet, a, b, changes);
                    }
                    Key::A if modifiers.command => {
                        let (r, c) = sheet.extent();
                        view.anchor = (0, 0);
                        view.cursor = (r.max(1) - 1, c.max(1) - 1);
                    }
                    _ => {}
                }
            }
            Event::Copy => {
                let (a, b) = view.selection();
                ctx.copy_text(copy_text(sheet, a, b));
            }
            Event::Cut => {
                let (a, b) = view.selection();
                ctx.copy_text(copy_text(sheet, a, b));
                clear(sheet, a, b, changes);
            }
            Event::Paste(text) => {
                let (a, _) = view.selection();
                let (rows, cols) = paste(sheet, a, &text, changes);
                view.anchor = a;
                view.cursor = (a.0 + rows - 1, a.1 + cols - 1);
            }
            Event::Text(text) if !text.chars().all(char::is_control) => {
                // Typing starts an edit that replaces the cell.
                view.edit = Some(Edit { text, frames: 0, caret_at_end: true });
                // The editor opens this frame; it mustn't see the same text again.
                ctx.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(_))));
            }
            _ => {}
        }
        if view.edit.is_some() {
            break;
        }
    }
}

/// Where Ctrl+arrow lands: the edge of the block of filled cells, or the next
/// filled cell past a gap, as spreadsheets do.
fn jump_to(sheet: &Sheet, from: Pos, dr: i64, dc: i64) -> u32 {
    let filled = |r: i64, c: i64| r >= 0 && c >= 0 && !sheet.input((r as u32, c as u32)).is_empty();
    let (mut r, mut c) = (from.0 as i64, from.1 as i64);
    let limit = |r: i64, c: i64| r >= 0 && c >= 0 && r < MAX_ROWS as i64 && c < MAX_COLS as i64;
    let (er, ec) = sheet.extent();
    let (er, ec) = (er as i64, ec as i64);
    let in_data = |r: i64, c: i64| r < er.max(1) && c < ec.max(1);
    let start_filled = filled(r, c);
    let next_filled = filled(r + dr, c + dc);
    if start_filled && next_filled {
        while limit(r + dr, c + dc) && filled(r + dr, c + dc) {
            r += dr;
            c += dc;
        }
    } else {
        loop {
            if !limit(r + dr, c + dc) || !in_data(r + dr, c + dc) && dr + dc > 0 {
                break;
            }
            r += dr;
            c += dc;
            if filled(r, c) {
                break;
            }
        }
    }
    if dr != 0 { r.max(0) as u32 } else { c.max(0) as u32 }
}

/// A one-line label for a range, for the status bar: `B2:B9` or `B2`.
pub fn range_name(a: Pos, b: Pos) -> String {
    if a == b { cell_name(a) } else { format!("{}:{}", cell_name(a), cell_name(b)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{PointerButton, RawInput};

    /// Runs the grid for a frame with these events, as egui would deliver them.
    struct Harness {
        ctx: egui::Context,
        time: f64,
        row_h: f32,
    }

    impl Harness {
        fn new() -> Self {
            Self { ctx: egui::Context::default(), time: 0.0, row_h: 0.0 }
        }

        fn frame(&mut self, sheet: &mut Sheet, view: &mut View, events: Vec<Event>) -> Changes {
            self.time += 0.1;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut changes = Vec::new();
            let mut row_h = 0.0;
            let _ = self.ctx.run_ui(raw, |ui| {
                row_h = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
                changes = show(ui, sheet, view, true);
            });
            self.row_h = row_h;
            sheet.recalc();
            changes
        }

        /// The middle of a cell, in screen coordinates (the grid starts at the origin).
        fn at(&self, (r, c): Pos) -> Pos2 {
            Pos2::new(HEADER_WIDTH + c as f32 * COL_WIDTH + 20.0, self.row_h * (r as f32 + 1.5))
        }

        fn click(&mut self, sheet: &mut Sheet, view: &mut View, pos: Pos2) {
            let btn = |pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(sheet, view, vec![Event::PointerMoved(pos)]);
            self.frame(sheet, view, vec![btn(true)]);
            self.frame(sheet, view, vec![btn(false)]);
        }

        fn key(&mut self, sheet: &mut Sheet, view: &mut View, key: Key, modifiers: Modifiers) -> Changes {
            self.frame(sheet, view, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }])
        }

        fn typed(&mut self, sheet: &mut Sheet, view: &mut View, text: &str) {
            self.frame(sheet, view, vec![Event::Text(text.into())]);
            // egui sends the rest of a typed word one frame at a time.
            self.frame(sheet, view, vec![]);
        }
    }

    #[test]
    fn clicking_then_typing_enters_a_value_and_enter_moves_down() {
        let (mut h, mut s, mut v) = (Harness::new(), Sheet::new(1, "S"), View::default());
        h.frame(&mut s, &mut v, vec![]);
        h.click(&mut s, &mut v, h.at((2, 1)));
        assert_eq!(v.cursor, (2, 1));
        h.typed(&mut s, &mut v, "12");
        assert!(v.edit.is_some());
        let changes = h.key(&mut s, &mut v, Key::Enter, Modifiers::NONE);
        assert_eq!(changes, vec![((2, 1), "12".to_string())]);
        assert_eq!(s.input((2, 1)), "12", "typing once gives the text once");
        assert_eq!(v.cursor, (3, 1));
        assert!(v.edit.is_none());
    }

    #[test]
    fn tab_commits_and_moves_right_escape_cancels() {
        let (mut h, mut s, mut v) = (Harness::new(), Sheet::new(1, "S"), View::default());
        h.frame(&mut s, &mut v, vec![]);
        h.typed(&mut s, &mut v, "Rent");
        h.key(&mut s, &mut v, Key::Tab, Modifiers::NONE);
        assert_eq!((s.input((0, 0)), v.cursor), ("Rent", (0, 1)));
        h.typed(&mut s, &mut v, "oops");
        h.key(&mut s, &mut v, Key::Escape, Modifiers::NONE);
        assert_eq!((s.input((0, 1)), v.cursor), ("", (0, 1)));
    }

    #[test]
    fn double_click_edits_what_is_there_and_clicking_away_keeps_it() {
        let (mut h, mut s, mut v) = (Harness::new(), Sheet::new(1, "S"), View::default());
        s.set((0, 0), "ab");
        h.frame(&mut s, &mut v, vec![]);
        let p = h.at((0, 0));
        h.click(&mut s, &mut v, p);
        h.click(&mut s, &mut v, p);
        assert!(v.edit.is_some(), "a double click starts editing");
        h.typed(&mut s, &mut v, "c");
        h.click(&mut s, &mut v, h.at((4, 3)));
        assert!(v.edit.is_none());
        assert_eq!(s.input((0, 0)), "abc");
        assert_eq!(v.cursor, (4, 3));
    }

    #[test]
    fn arrows_select_and_delete_clears() {
        let (mut h, mut s, mut v) = (Harness::new(), Sheet::new(1, "S"), View::default());
        for r in 0..3 {
            s.set((r, 0), "1");
        }
        h.frame(&mut s, &mut v, vec![]);
        h.key(&mut s, &mut v, Key::ArrowDown, Modifiers::SHIFT);
        h.key(&mut s, &mut v, Key::ArrowDown, Modifiers::SHIFT);
        assert_eq!(v.selection(), ((0, 0), (2, 0)));
        let changes = h.key(&mut s, &mut v, Key::Delete, Modifiers::NONE);
        assert_eq!(changes.len(), 3);
        assert!(s.is_empty());
    }

    #[test]
    fn copy_paste_round_trip() {
        let mut s = Sheet::new(1, "S");
        let mut ch = Vec::new();
        let (rows, cols) = paste(&mut s, (1, 1), "a\t1\nb\t=C2*2\n", &mut ch);
        s.recalc();
        assert_eq!((rows, cols), (2, 2));
        assert_eq!(s.input((2, 2)), "=C2*2");
        assert_eq!(copy_text(&s, (1, 1), (2, 2)), "a\t1\nb\t2");
        assert_eq!(ch.len(), 4);
    }

    #[test]
    fn range_names() {
        assert_eq!(range_name((0, 0), (0, 0)), "A1");
        assert_eq!(range_name((1, 1), (8, 1)), "B2:B9");
    }

    #[test]
    fn ctrl_arrow_jumps() {
        let mut s = Sheet::new(1, "S");
        for r in 2..5 {
            s.set((r, 0), "1");
        }
        assert_eq!(jump_to(&s, (2, 0), 1, 0), 4);
        assert_eq!(jump_to(&s, (4, 0), -1, 0), 2);
        assert_eq!(jump_to(&s, (0, 0), 1, 0), 2);
    }
}
