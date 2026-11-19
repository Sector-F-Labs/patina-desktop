//! Indenting: what Tab, Shift+Tab and Enter do in the editor, and a
//! file's own indentation, guessed from its lines. Positions are in
//! characters, as egui's cursors count them.

use std::ops::Range;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Indent {
    pub spaces: bool,
    pub width: usize,
}

impl Default for Indent {
    fn default() -> Self {
        Indent { spaces: true, width: 4 }
    }
}

impl Indent {
    /// One level, as typed at the start of a line.
    fn unit(&self) -> String {
        if self.spaces { " ".repeat(self.width) } else { "\t".into() }
    }

    pub fn label(&self) -> String {
        if self.spaces { format!("Spaces: {}", self.width) } else { format!("Tabs: {}", self.width) }
    }
}

/// The byte where character `at` starts (the end for one past it).
pub fn byte_at(text: &str, at: usize) -> usize {
    text.char_indices().nth(at).map_or(text.len(), |(i, _)| i)
}

/// How many characters come before byte `at`.
pub fn char_at(text: &str, at: usize) -> usize {
    text[..at.min(text.len())].chars().count()
}

/// The indentation most of a file's indented lines use, if it has any:
/// tabs, or spaces in steps of the commonest increase between lines.
pub fn detect(text: &str) -> Option<Indent> {
    let (mut tabs, mut spaced) = (0, 0);
    let mut steps = [0usize; 9];
    let mut previous = 0;
    for line in text.lines().take(5000).filter(|l| !l.trim().is_empty()) {
        if line.starts_with('\t') {
            tabs += 1;
            continue;
        }
        let n = line.len() - line.trim_start_matches(' ').len();
        if n > 0 {
            spaced += 1;
        }
        if n > previous && n - previous < steps.len() {
            steps[n - previous] += 1;
        }
        previous = n;
    }
    if tabs == 0 && spaced == 0 {
        return None;
    }
    if tabs > spaced {
        return Some(Indent { spaces: false, width: 8 });
    }
    let width = (2..steps.len()).max_by_key(|&w| (steps[w], w == 4)).filter(|&w| steps[w] > 0)?;
    Some(Indent { spaces: true, width })
}

/// The lines `range` (bytes) touches, as the bytes each starts at.
fn line_starts(text: &str, range: &Range<usize>) -> Vec<usize> {
    let first = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
    let mut starts = vec![first];
    // A selection ending at a line's start doesn't take that line in.
    let last = if range.end > range.start && text[..range.end].ends_with('\n') { range.end - 1 } else { range.end };
    starts.extend(text[first..last].match_indices('\n').map(|(i, _)| first + i + 1));
    starts
}

/// The text after Tab (or Shift+Tab) over the selection `sel`, and the
/// selection after it. A selection across lines, or Shift, indents or
/// outdents whole lines; otherwise Tab puts in one level at the cursor.
pub fn tab(text: &str, sel: Range<usize>, indent: Indent, outdent: bool) -> (String, Range<usize>) {
    let bytes = byte_at(text, sel.start)..byte_at(text, sel.end);
    let across = text[bytes.clone()].contains('\n');
    if !outdent && !across {
        let column = char_at(text, bytes.start) - char_at(text, text[..bytes.start].rfind('\n').map_or(0, |i| i + 1));
        let unit = if indent.spaces { " ".repeat(indent.width - column % indent.width) } else { "\t".into() };
        let mut out = String::with_capacity(text.len() + unit.len());
        out.push_str(&text[..bytes.start]);
        out.push_str(&unit);
        out.push_str(&text[bytes.end..]);
        let at = sel.start + unit.chars().count();
        return (out, at..at);
    }
    let starts = line_starts(text, &bytes);
    let mut out = String::with_capacity(text.len() + starts.len() * indent.width);
    let mut last = 0;
    // Characters added (or taken) before the selection's start and end.
    let (mut before_start, mut before_end) = (0isize, 0isize);
    for &start in &starts {
        out.push_str(&text[last..start]);
        let line = &text[start..];
        let change = if outdent {
            let n = if line.starts_with('\t') {
                1
            } else {
                line.chars().take(indent.width).take_while(|&c| c == ' ').count()
            };
            last = start + n;
            -(n as isize)
        } else {
            let blank = line.split('\n').next().is_none_or(|l| l.trim().is_empty());
            if !blank {
                out.push_str(&indent.unit());
            }
            last = start;
            if blank { 0 } else { indent.unit().chars().count() as isize }
        };
        // A removed indent the selection started inside moves it to the line's start.
        let shift = |at: usize| {
            if change < 0 && at < start + change.unsigned_abs() { start as isize - at as isize } else { change }
        };
        if start <= bytes.start {
            before_start += shift(bytes.start);
        }
        if start <= bytes.end {
            before_end += shift(bytes.end);
        }
    }
    out.push_str(&text[last..]);
    let moved = |at: usize, by: isize| (at as isize + by).max(0) as usize;
    (out, moved(sel.start, before_start)..moved(sel.end, before_end))
}

/// The text after Enter over the selection `sel`: a new line, indented as
/// the one it was typed in, and where the cursor goes.
pub fn newline(text: &str, sel: Range<usize>) -> (String, usize) {
    let bytes = byte_at(text, sel.start)..byte_at(text, sel.end);
    let line = text[..bytes.start].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[line..bytes.start].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let mut out = String::with_capacity(text.len() + indent.len() + 1);
    out.push_str(&text[..bytes.start]);
    out.push('\n');
    out.push_str(&indent);
    out.push_str(&text[bytes.end..]);
    (out, sel.start + 1 + indent.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUR: Indent = Indent { spaces: true, width: 4 };

    #[test]
    fn guesses_a_files_indentation() {
        assert_eq!(detect("fn a() {\n    b();\n    if c {\n        d();\n    }\n}\n"), Some(FOUR));
        assert_eq!(detect("a:\n  b:\n    c: 1\n  d: 2\n"), Some(Indent { spaces: true, width: 2 }));
        assert_eq!(detect("int f() {\n\treturn 1;\n}\n"), Some(Indent { spaces: false, width: 8 }));
        assert_eq!(detect("plain\ntext\n"), None);
    }

    #[test]
    fn tab_at_the_cursor_reaches_the_next_stop() {
        assert_eq!(tab("ab", 2..2, FOUR, false), ("ab  ".into(), 4..4));
        assert_eq!(tab("abcd", 0..0, FOUR, false), ("    abcd".into(), 4..4));
        assert_eq!(tab("é", 1..1, Indent { spaces: false, width: 8 }, false), ("é\t".into(), 2..2));
        // A selection on one line is replaced, as typing would.
        assert_eq!(tab("abcd", 1..3, FOUR, false), ("a   d".into(), 4..4));
    }

    #[test]
    fn tab_over_lines_indents_them() {
        let (text, sel) = tab("a\n\nb\nc", 0..4, FOUR, false);
        assert_eq!(text, "    a\n\n    b\nc");
        assert_eq!(sel, 4..12);
        // Ending at a line's start leaves that line be.
        let (text, _) = tab("a\nb\n", 0..2, FOUR, false);
        assert_eq!(text, "    a\nb\n");
    }

    #[test]
    fn shift_tab_outdents() {
        let (text, sel) = tab("    a\n  b\n\tc", 0..12, FOUR, true);
        assert_eq!(text, "a\nb\nc");
        assert_eq!(sel, 0..5);
        // The cursor inside the indent goes to the line's start.
        assert_eq!(tab("    a", 2..2, FOUR, true), ("a".into(), 0..0));
        assert_eq!(tab("    ab", 6..6, FOUR, true), ("ab".into(), 2..2));
        assert_eq!(tab("x", 1..1, FOUR, true), ("x".into(), 1..1));
    }

    #[test]
    fn enter_keeps_the_indent() {
        assert_eq!(newline("    a", 5..5), ("    a\n    ".into(), 10));
        assert_eq!(newline("\tif x", 5..5), ("\tif x\n\t".into(), 7));
        assert_eq!(newline("ab", 1..2), ("a\n".into(), 2));
        // Enter inside the indent takes only what's before the cursor.
        assert_eq!(newline("    a", 2..2), ("  \n    a".into(), 5));
    }
}
