//! The open picture with its undo history, and the tools that change it.
//!
//! A stroke is press, drag, release (the app calls [`Document::begin`],
//! [`Document::drag`] and [`Document::end`]). Shapes are redrawn from the
//! picture as it was at the press on every drag, so they can be stretched
//! before they are let go; the release is one undo step.

use std::path::PathBuf;

use crate::bitmap::{Bitmap, Fill, Rgba};

/// How many steps back undo goes.
const HISTORY: usize = 50;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pencil,
    Brush,
    Eraser,
    Fill,
    Picker,
    Line,
    Rectangle,
    Ellipse,
}

impl Tool {
    pub const ALL: [Tool; 8] =
        [Tool::Pencil, Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Picker, Tool::Line, Tool::Rectangle, Tool::Ellipse];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Pencil => "Pencil",
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Fill => "Fill",
            Tool::Picker => "Picker",
            Tool::Line => "Line",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
        }
    }

    pub fn key(self) -> egui::Key {
        match self {
            Tool::Pencil => egui::Key::P,
            Tool::Brush => egui::Key::B,
            Tool::Eraser => egui::Key::E,
            Tool::Fill => egui::Key::F,
            Tool::Picker => egui::Key::K,
            Tool::Line => egui::Key::L,
            Tool::Rectangle => egui::Key::R,
            Tool::Ellipse => egui::Key::O,
        }
    }

    pub fn is_shape(self) -> bool {
        matches!(self, Tool::Line | Tool::Rectangle | Tool::Ellipse)
    }
}

/// What the next stroke draws with.
#[derive(Clone, Copy, Debug)]
pub struct Pen {
    pub tool: Tool,
    pub size: u32,
    pub fill: Fill,
    pub fg: Rgba,
    pub bg: Rgba,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Primary,
    Secondary,
}

struct Stroke {
    before: Bitmap,
    tool: Tool,
    start: (i32, i32),
    last: (i32, i32),
    /// Outline, then inside: the pen's colours, swapped for the right button.
    colours: (Rgba, Rgba),
}

pub struct Document {
    pub bitmap: Bitmap,
    pub path: Option<PathBuf>,
    pub modified: bool,
    undo: Vec<Bitmap>,
    redo: Vec<Bitmap>,
    stroke: Option<Stroke>,
}

impl Document {
    pub fn new(bitmap: Bitmap) -> Self {
        Self { bitmap, path: None, modified: false, undo: Vec::new(), redo: Vec::new(), stroke: None }
    }

    pub fn drawing(&self) -> bool {
        self.stroke.is_some()
    }

    /// Starts a stroke at `pos`. The Picker doesn't draw: it answers with the
    /// colour it found.
    pub fn begin(&mut self, pen: &Pen, button: Button, pos: (i32, i32)) -> Option<Rgba> {
        if pen.tool == Tool::Picker {
            return self.bitmap.get(pos.0, pos.1);
        }
        let colours = if button == Button::Primary { (pen.fg, pen.bg) } else { (pen.bg, pen.fg) };
        // The eraser paints the background colour, as Paint's does.
        let ink = if pen.tool == Tool::Eraser { colours.1 } else { colours.0 };
        let before = self.bitmap.clone();
        match pen.tool {
            Tool::Pencil => self.bitmap.dab(pos.0, pos.1, 1, ink),
            Tool::Brush | Tool::Eraser => self.bitmap.dab(pos.0, pos.1, pen.size, ink),
            Tool::Fill => self.bitmap.flood(pos.0, pos.1, ink),
            _ => {}
        }
        self.stroke = Some(Stroke { before, tool: pen.tool, start: pos, last: pos, colours });
        if pen.tool == Tool::Fill {
            self.end();
        }
        None
    }

    /// Carries the stroke to `pos`; `constrain` (Shift) keeps lines to 45°
    /// steps and rectangles and ellipses square.
    pub fn drag(&mut self, pen: &Pen, pos: (i32, i32), constrain: bool) {
        let Some(stroke) = &mut self.stroke else { return };
        let ink = if stroke.tool == Tool::Eraser { stroke.colours.1 } else { stroke.colours.0 };
        match stroke.tool {
            Tool::Pencil => self.bitmap.line(stroke.last, pos, 1, ink),
            Tool::Brush | Tool::Eraser => self.bitmap.line(stroke.last, pos, pen.size, ink),
            Tool::Line | Tool::Rectangle | Tool::Ellipse => {
                let end = if constrain { constrained(stroke.tool, stroke.start, pos) } else { pos };
                self.bitmap = stroke.before.clone();
                let (a, size) = (stroke.start, pen.size);
                let (outline, inside) = stroke.colours;
                match stroke.tool {
                    Tool::Line => self.bitmap.line(a, end, size, outline),
                    Tool::Rectangle => self.bitmap.rectangle(a, end, size, pen.fill, outline, inside),
                    _ => self.bitmap.ellipse(a, end, size, pen.fill, outline, inside),
                }
            }
            Tool::Fill | Tool::Picker => {}
        }
        stroke.last = pos;
    }

    /// Lets go: a stroke that changed something becomes an undo step.
    pub fn end(&mut self) {
        let Some(stroke) = self.stroke.take() else { return };
        if self.bitmap != stroke.before {
            self.push_undo(stroke.before);
        }
    }

    /// Drops the stroke in progress and puts the picture back.
    pub fn cancel(&mut self) {
        if let Some(stroke) = self.stroke.take() {
            self.bitmap = stroke.before;
        }
    }

    /// A change to the whole picture (flip, resize, clear), as one undo step.
    pub fn edit(&mut self, change: impl FnOnce(&mut Bitmap)) {
        self.cancel();
        let before = self.bitmap.clone();
        change(&mut self.bitmap);
        if self.bitmap != before {
            self.push_undo(before);
        }
    }

    pub fn undo(&mut self) -> bool {
        self.cancel();
        let Some(back) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(&mut self.bitmap, back));
        self.modified = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        self.cancel();
        let Some(forward) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(&mut self.bitmap, forward));
        self.modified = true;
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn push_undo(&mut self, before: Bitmap) {
        self.undo.push(before);
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.modified = true;
    }
}

fn constrained(tool: Tool, start: (i32, i32), pos: (i32, i32)) -> (i32, i32) {
    let (dx, dy) = (pos.0 - start.0, pos.1 - start.1);
    if tool == Tool::Line {
        let (ax, ay) = (dx.abs(), dy.abs());
        // Within 22.5° of an axis it runs along it, else it goes diagonally.
        if ay * 5 < ax * 2 {
            (pos.0, start.1)
        } else if ax * 5 < ay * 2 {
            (start.0, pos.1)
        } else {
            let n = ax.max(ay);
            (start.0 + n * dx.signum(), start.1 + n * dy.signum())
        }
    } else {
        let n = dx.abs().max(dy.abs());
        (start.0 + n * dx.signum(), start.1 + n * dy.signum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::WHITE;

    const BLACK: Rgba = [0, 0, 0, 255];
    const RED: Rgba = [255, 0, 0, 255];

    fn pen(tool: Tool) -> Pen {
        Pen { tool, size: 3, fill: Fill::Outline, fg: BLACK, bg: RED }
    }

    fn doc() -> Document {
        Document::new(Bitmap::new(20, 20, WHITE))
    }

    fn stroke(d: &mut Document, p: &Pen, button: Button, from: (i32, i32), to: (i32, i32)) {
        d.begin(p, button, from);
        d.drag(p, to, false);
        d.end();
    }

    #[test]
    fn a_stroke_is_one_undo_step() {
        let mut d = doc();
        let p = pen(Tool::Pencil);
        stroke(&mut d, &p, Button::Primary, (1, 1), (10, 1));
        assert_eq!(d.bitmap.get(5, 1), Some(BLACK));
        assert!(d.modified);
        assert!(d.undo());
        assert_eq!(d.bitmap.get(5, 1), Some(WHITE));
        assert!(!d.can_undo());
        assert!(d.redo());
        assert_eq!(d.bitmap.get(5, 1), Some(BLACK));
        assert!(!d.redo());
    }

    #[test]
    fn a_new_stroke_clears_redo() {
        let mut d = doc();
        let p = pen(Tool::Pencil);
        stroke(&mut d, &p, Button::Primary, (1, 1), (3, 1));
        d.undo();
        assert!(d.can_redo());
        stroke(&mut d, &p, Button::Primary, (1, 5), (3, 5));
        assert!(!d.can_redo());
    }

    #[test]
    fn the_right_button_swaps_the_colours() {
        let mut d = doc();
        stroke(&mut d, &pen(Tool::Pencil), Button::Secondary, (1, 1), (4, 1));
        assert_eq!(d.bitmap.get(2, 1), Some(RED));
    }

    #[test]
    fn a_stroke_that_changes_nothing_is_not_an_undo_step() {
        let mut d = doc();
        let mut p = pen(Tool::Fill);
        p.fg = WHITE;
        stroke(&mut d, &p, Button::Primary, (3, 3), (3, 3));
        assert!(!d.can_undo());
        assert!(!d.modified);
    }

    #[test]
    fn fill_acts_on_the_press() {
        let mut d = doc();
        d.begin(&pen(Tool::Fill), Button::Primary, (3, 3));
        assert!(!d.drawing());
        assert_eq!(d.bitmap.get(19, 19), Some(BLACK));
        assert!(d.can_undo());
    }

    #[test]
    fn the_eraser_paints_the_background() {
        let mut d = doc();
        d.bitmap.put(5, 5, BLACK);
        let mut p = pen(Tool::Eraser);
        p.bg = WHITE;
        stroke(&mut d, &p, Button::Primary, (5, 5), (5, 5));
        assert_eq!(d.bitmap.get(5, 5), Some(WHITE));
    }

    #[test]
    fn the_picker_reads_and_never_draws() {
        let mut d = doc();
        d.bitmap.put(2, 2, RED);
        assert_eq!(d.begin(&pen(Tool::Picker), Button::Primary, (2, 2)), Some(RED));
        assert_eq!(d.begin(&pen(Tool::Picker), Button::Primary, (-4, 2)), None);
        assert!(!d.drawing() && !d.can_undo());
    }

    #[test]
    fn a_shape_follows_the_pointer_without_leaving_trails() {
        let mut d = doc();
        let p = pen(Tool::Rectangle);
        d.begin(&p, Button::Primary, (2, 2));
        d.drag(&p, (15, 15), false);
        d.drag(&p, (6, 6), false);
        d.end();
        assert_eq!(d.bitmap.get(15, 15), Some(WHITE));
        assert_eq!(d.bitmap.get(6, 6), Some(BLACK));
        assert!(d.undo());
        assert_eq!(d.bitmap, Bitmap::new(20, 20, WHITE));
    }

    #[test]
    fn cancelling_puts_the_picture_back() {
        let mut d = doc();
        let p = pen(Tool::Line);
        d.begin(&p, Button::Primary, (0, 0));
        d.drag(&p, (10, 10), false);
        d.cancel();
        assert_eq!(d.bitmap, Bitmap::new(20, 20, WHITE));
        assert!(!d.can_undo());
    }

    #[test]
    fn shift_keeps_lines_to_45_degrees_and_boxes_square() {
        assert_eq!(constrained(Tool::Line, (5, 5), (15, 6)), (15, 5));
        assert_eq!(constrained(Tool::Line, (5, 5), (6, 15)), (5, 15));
        assert_eq!(constrained(Tool::Line, (5, 5), (12, 10)), (12, 12));
        assert_eq!(constrained(Tool::Line, (5, 5), (0, 0)), (0, 0));
        assert_eq!(constrained(Tool::Rectangle, (5, 5), (15, 8)), (15, 15));
        assert_eq!(constrained(Tool::Ellipse, (5, 5), (2, 9)), (1, 9));
    }

    #[test]
    fn whole_picture_edits_undo() {
        let mut d = doc();
        d.edit(|b| b.resize_canvas(5, 5, WHITE));
        assert_eq!((d.bitmap.width, d.bitmap.height), (5, 5));
        d.edit(|b| b.flip_horizontal()); // nothing to flip: no step
        d.undo();
        assert_eq!((d.bitmap.width, d.bitmap.height), (20, 20));
    }

    #[test]
    fn history_is_capped() {
        let mut d = doc();
        let p = pen(Tool::Pencil);
        for i in 0..HISTORY as i32 + 10 {
            stroke(&mut d, &p, Button::Primary, (i % 20, 0), (i % 20, 1));
            d.edit(|b| b.invert());
        }
        assert_eq!(d.undo.len(), HISTORY);
    }
}
