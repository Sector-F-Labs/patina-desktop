//! The editing area: egui's `TextEdit` (ADR-0002) with the highlighter's
//! colours, found text marked, line numbers beside it and the cursor's
//! line lit. Tab and Shift+Tab indent, Enter keeps the line's indent.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::Duration;

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{Align, Align2, FontId, Id, Key, Modifiers, Sense, Shape, Ui, vec2};

use crate::document::Document;
use crate::find;
use crate::indent;

/// How long a frame may spend colouring before it draws.
const COLOURING: Duration = Duration::from_millis(4);
/// Space either side of the line numbers.
const GUTTER_PAD: f32 = 10.0;

pub struct Look {
    pub line_numbers: bool,
    pub wrap: bool,
    pub highlight_line: bool,
    pub font: FontId,
}

/// What happened in the editor this frame.
#[derive(Default)]
pub struct Shown {
    pub changed: bool,
    /// The selection, in characters, start before end.
    pub selection: Option<Range<usize>>,
}

/// The editor's widget id, for its state in egui's memory.
pub fn text_id(doc: &Document) -> Id {
    Id::new(("text", doc.id))
}

/// The selection, in characters, start before end.
pub fn selection(ctx: &egui::Context, doc: &Document) -> Option<Range<usize>> {
    let range = TextEditState::load(ctx, text_id(doc))?.cursor.char_range()?;
    let [a, b] = [range.primary.index.0, range.secondary.index.0];
    Some(a.min(b)..a.max(b))
}

/// Selects `range` (characters), the cursor at its end.
pub fn select(ctx: &egui::Context, doc: &Document, range: Range<usize>) {
    let id = text_id(doc);
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(range.start), CCursor::new(range.end))));
    state.store(ctx, id);
}

/// Keeps the text as it is now as an undo step, before the program
/// changes it (Replace All, a file read again), so Undo brings it back.
pub fn checkpoint(ctx: &egui::Context, doc: &Document) {
    let id = text_id(doc);
    let Some(state) = TextEditState::load(ctx, id) else { return };
    let range = state.cursor.char_range().unwrap_or_default();
    let mut undoer = state.undoer();
    undoer.add_undo(&(range, doc.text.clone()));
    let mut state = state;
    state.set_undoer(undoer);
    state.store(ctx, id);
}

/// Forgets the undo steps, for text newly read from a file.
pub fn forget_undo(ctx: &egui::Context, doc: &Document) {
    let id = text_id(doc);
    if let Some(mut state) = TextEditState::load(ctx, id) {
        state.clear_undoer();
        state.store(ctx, id);
    }
}

/// Whether there's a step to undo, and one to redo.
pub fn can_undo(ctx: &egui::Context, doc: &Document) -> (bool, bool) {
    let Some(state) = TextEditState::load(ctx, text_id(doc)) else { return (false, false) };
    let current = (state.cursor.char_range().unwrap_or_default(), doc.text.clone());
    let undoer = state.undoer();
    (undoer.has_undo(&current), undoer.has_redo(&current))
}

/// Undoes (or redoes) a step, as Ctrl+Z does. Whether there was one.
pub fn undo(ctx: &egui::Context, doc: &mut Document, redo: bool) -> bool {
    let id = text_id(doc);
    let Some(mut state) = TextEditState::load(ctx, id) else { return false };
    let mut undoer = state.undoer();
    let current = (state.cursor.char_range().unwrap_or_default(), doc.text.clone());
    let step = if redo { undoer.redo(&current) } else { undoer.undo(&current) };
    let Some((range, text)) = step.cloned() else { return false };
    doc.text = text;
    state.cursor.set_char_range(Some(range));
    state.set_undoer(undoer);
    state.store(ctx, id);
    doc.edited();
    true
}

/// Tab, Shift+Tab and Enter, taken before the `TextEdit` sees them.
fn indent_keys(ui: &Ui, doc: &mut Document) -> bool {
    let id = text_id(doc);
    if doc.read_only.is_some() || !ui.memory(|m| m.has_focus(id)) {
        return false;
    }
    // Shift first: a key without modifiers matches it with Shift too.
    let (outdent, tab, enter) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::SHIFT, Key::Tab),
            i.consume_key(Modifiers::NONE, Key::Tab),
            !i.events.iter().any(|e| matches!(e, egui::Event::Ime(_))) && i.consume_key(Modifiers::NONE, Key::Enter),
        )
    });
    if !(tab || outdent || enter) {
        return false;
    }
    let sel = selection(ui.ctx(), doc).unwrap_or_else(|| {
        let end = doc.text.chars().count();
        end..end
    });
    let after = if enter {
        let (text, at) = indent::newline(&doc.text, sel);
        doc.text = text;
        at..at
    } else {
        let (text, after) = indent::tab(&doc.text, sel, doc.indent, outdent);
        doc.text = text;
        after
    };
    select(ui.ctx(), doc, after);
    true
}

/// Draws the document, taking typing, clicks and the context menu.
/// `scroll_to` (characters) is brought into view.
pub fn show(ui: &mut Ui, doc: &mut Document, look: &Look, focus: bool, scroll_to: Option<usize>) -> Shown {
    let id = text_id(doc);
    let ctx = ui.ctx().clone();
    let mut shown = Shown { changed: indent_keys(ui, doc), ..Shown::default() };
    if let Some(range) = doc.place_cursor.take() {
        let end = doc.text.chars().count();
        select(&ctx, doc, range.start.min(end)..range.end.min(end));
    }
    let language = doc.language();
    doc.highlighter.set(language, ui.visuals().dark_mode);

    let visuals = ui.visuals().clone();
    let plain = visuals.text_color();
    let (mark, strong) = (visuals.selection.bg_fill.gamma_multiply(0.35), visuals.selection.bg_fill);
    let wrap = look.wrap;
    let font = look.font.clone();
    let Document { text, highlighter, galley: cache, found, current, read_only, .. } = doc;
    let (found, current) = (&*found, *current);
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, width: f32| {
        highlighter.update(buf.as_str());
        highlighter.advance(COLOURING);
        let mut key = std::collections::hash_map::DefaultHasher::new();
        (highlighter.generation(), wrap.then_some(width.to_bits()), font.size.to_bits(), plain, mark, strong)
            .hash(&mut key);
        (found.len(), found.first(), found.last(), current).hash(&mut key);
        let key = key.finish();
        if let Some((k, galley)) = cache.as_ref()
            && *k == key
        {
            return galley.clone();
        }
        let mut job = highlighter.job(&font, plain);
        job.wrap.max_width = if wrap { width } else { f32::INFINITY };
        find::mark(&mut job, found, current, mark, strong);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        *cache = Some((key, galley.clone()));
        galley
    };

    let lines = text.bytes().filter(|&b| b == b'\n').count() + 1;
    let digit = ui.fonts_mut(|f| f.glyph_width(&look.font, '0'));
    let gutter = if look.line_numbers { lines.to_string().len().max(2) as f32 * digit + 2.0 * GUTTER_PAD } else { 6.0 };
    let scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
    scroll.id_salt(("scroll", id)).auto_shrink(false).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (gutter_rect, _) = ui.allocate_exact_size(vec2(gutter, ui.available_height()), Sense::hover());
            let backdrop = ui.painter().add(Shape::Noop);
            let mut read = text.as_str();
            let buffer: &mut dyn egui::TextBuffer = if read_only.is_some() { &mut read } else { text };
            let output = egui::TextEdit::multiline(buffer)
                .id(id)
                .font(look.font.clone())
                .lock_focus(true)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::symmetric(0, 6))
                .desired_width(f32::INFINITY)
                .min_size(ui.available_size())
                .layouter(&mut layouter)
                .show(ui);
            let response = &output.response.response;
            shown.changed |= response.changed();
            if focus && !response.has_focus() {
                response.request_focus();
            }
            let galley = &output.galley;
            let origin = output.galley_pos;
            let cursor = output.cursor_range.map(|r| r.primary);
            // The cursor's row, lit across the editor and its numbers.
            if look.highlight_line
                && response.has_focus()
                && let Some(cursor) = cursor
            {
                let row = galley.pos_from_cursor(cursor).translate(origin.to_vec2());
                let lit = egui::Rect::from_x_y_ranges(gutter_rect.left()..=ui.clip_rect().right(), row.y_range());
                ui.painter().set(backdrop, Shape::rect_filled(lit, 0.0, visuals.widgets.noninteractive.weak_bg_fill));
            }
            if look.line_numbers {
                paint_numbers(ui, galley, origin, gutter_rect, &look.font, cursor, &visuals);
            }
            if let Some(at) = scroll_to {
                let rect = galley.pos_from_cursor(CCursor::new(at)).translate(origin.to_vec2());
                ui.scroll_to_rect(rect.expand2(vec2(40.0, 60.0)), Some(Align::Center));
            }
            context_menu(response, id, read_only.is_some());
        });
    });
    if highlighter.pending() {
        ui.ctx().request_repaint();
    }
    if shown.changed {
        doc.edited();
    }
    shown.selection = selection(&ctx, doc);
    shown
}

/// Each line's number beside its first row, the cursor's line in full
/// text colour.
fn paint_numbers(
    ui: &Ui,
    galley: &egui::Galley,
    origin: egui::Pos2,
    gutter: egui::Rect,
    font: &FontId,
    cursor: Option<CCursor>,
    visuals: &egui::Visuals,
) {
    let clip = ui.clip_rect();
    let cursor_line = cursor.map(|c| line_of(galley, galley.layout_from_cursor(c).row));
    let mut line = 1;
    let mut starts_line = true;
    for row in &galley.rows {
        let rect = row.rect().translate(origin.to_vec2());
        if rect.top() > clip.bottom() {
            break;
        }
        if starts_line && rect.bottom() >= clip.top() {
            let colour = if cursor_line == Some(line) { visuals.text_color() } else { visuals.weak_text_color() };
            let at = egui::pos2(gutter.right() - GUTTER_PAD, rect.top());
            ui.painter().text(at, Align2::RIGHT_TOP, line.to_string(), font.clone(), colour);
        }
        starts_line = row.ends_with_newline;
        if starts_line {
            line += 1;
        }
    }
}

/// The line (from 1) a galley row is part of.
fn line_of(galley: &egui::Galley, row: usize) -> usize {
    1 + galley.rows[..row].iter().filter(|r| r.ends_with_newline).count()
}

/// Cut, Copy, Paste and Select All on a right click.
fn context_menu(response: &egui::Response, id: Id, read_only: bool) {
    patina::menu::context(response, |ui| {
        let ctx = ui.ctx().clone();
        let mut command = None;
        if ui.add_enabled(!read_only, egui::Button::new("Cut").shortcut_text("Ctrl+X")).clicked() {
            command = Some(egui::ViewportCommand::RequestCut);
        }
        if ui.add(egui::Button::new("Copy").shortcut_text("Ctrl+C")).clicked() {
            command = Some(egui::ViewportCommand::RequestCopy);
        }
        if ui.add_enabled(!read_only, egui::Button::new("Paste").shortcut_text("Ctrl+V")).clicked() {
            command = Some(egui::ViewportCommand::RequestPaste);
        }
        ui.separator();
        if ui.add(egui::Button::new("Select All").shortcut_text("Ctrl+A")).clicked() {
            let mut state = TextEditState::load(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(usize::MAX))));
            state.store(&ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
        }
        if let Some(command) = command {
            ctx.send_viewport_cmd(command);
            // The menu took the focus; the editor needs it for the command.
            ctx.memory_mut(|m| m.request_focus(id));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indent::Indent;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect};

    struct Harness {
        ctx: egui::Context,
        doc: Document,
        time: f64,
    }

    impl Harness {
        fn new(text: &str) -> Self {
            let mut doc = Document::new(1, Indent::default());
            doc.text = text.to_string();
            Harness { ctx: egui::Context::default(), doc, time: 0.0 }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 0.1;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                events,
                time: Some(self.time),
                ..Default::default()
            };
            let look = Look { line_numbers: true, wrap: true, highlight_line: true, font: FontId::monospace(14.0) };
            let doc = &mut self.doc;
            let _ = self.ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, doc, &look, false, None);
                });
            });
        }

        fn click(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
            self.frame(vec![event(true), event(false)]);
        }

        fn type_text(&mut self, text: &str) {
            self.frame(vec![Event::Text(text.into())]);
        }
    }

    /// A point far right on the first line, so a click puts the cursor at
    /// that line's end.
    const LINE_ONE_END: Pos2 = Pos2 { x: 700.0, y: 20.0 };

    #[test]
    fn a_click_then_typing_edits() {
        let mut h = Harness::new("hello\nworld");
        h.frame(vec![]);
        h.click(LINE_ONE_END);
        h.type_text("!");
        assert_eq!(h.doc.text, "hello!\nworld");
        assert!(h.doc.modified);
    }

    #[test]
    fn enter_keeps_the_indent_and_tab_indents() {
        let mut h = Harness::new("    let x;");
        h.frame(vec![]);
        h.click(LINE_ONE_END);
        h.key(Key::Enter, Modifiers::NONE);
        h.type_text("y");
        assert_eq!(h.doc.text, "    let x;\n    y");
        h.key(Key::Tab, Modifiers::NONE);
        assert_eq!(h.doc.text, "    let x;\n    y   ");
        h.key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(h.doc.text, "    let x;\ny   ");
    }

    #[test]
    fn undo_steps_back() {
        let mut h = Harness::new("a");
        h.frame(vec![]);
        h.click(LINE_ONE_END);
        h.type_text("b");
        // egui keeps an undo step once typing has paused.
        h.time += 2.0;
        h.frame(vec![]);
        assert!(undo(&h.ctx, &mut h.doc, false));
        assert_eq!(h.doc.text, "a");
        assert!(undo(&h.ctx, &mut h.doc, true));
        assert_eq!(h.doc.text, "ab");
    }

    #[test]
    fn read_only_text_takes_no_typing() {
        let mut h = Harness::new("fixed");
        h.doc.read_only = Some("because".into());
        h.frame(vec![]);
        h.click(LINE_ONE_END);
        h.type_text("x");
        h.key(Key::Tab, Modifiers::NONE);
        assert_eq!(h.doc.text, "fixed");
    }
}
