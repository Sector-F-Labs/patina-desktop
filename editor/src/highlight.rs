//! Syntax highlighting with syntect, a line at a time and kept between
//! edits (ADR-0003): an edit recolours from its line on, only until the
//! parser's state meets what it was before, and a frame does a few
//! milliseconds of that at most, so a long file never stalls typing. Lines
//! not reached yet keep their old colours.

use std::ops::Range;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use egui::text::{ByteIndex, LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontId, Stroke};
use syntect::highlighting::{FontStyle, HighlightState, Highlighter as Styler, RangedHighlightIterator, Theme};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Files larger than this are shown plain: laying them out costs enough.
pub const MOST_BYTES: usize = 4 << 20;
/// Lines longer than this (minified files) are shown plain, and the parser
/// passes over them as it found them.
const LONGEST_LINE: usize = 20_000;

/// The syntaxes, loaded once: bat's set, which is syntect's own and more
/// (TOML, TypeScript, Dockerfile...) (ADR-0009).
pub fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The themes Patina's code blocks use (egui_extras' defaults), so code
/// looks the same in every Patina app.
fn theme(dark: bool) -> &'static Theme {
    static THEMES: OnceLock<syntect::highlighting::ThemeSet> = OnceLock::new();
    let set = THEMES.get_or_init(syntect::highlighting::ThemeSet::load_defaults);
    &set.themes[if dark { "base16-mocha.dark" } else { "Solarized (light)" }]
}

/// The syntax for a file: by its name, else its first line (`#!/bin/sh`).
pub fn detect(name: Option<&str>, text: &str) -> Option<String> {
    let set = syntaxes();
    let by_name = name.and_then(|name| {
        let path = std::path::Path::new(name);
        let ext = path.extension().and_then(|e| e.to_str());
        let file = path.file_name().and_then(|f| f.to_str());
        ext.and_then(|e| set.find_syntax_by_extension(e)).or_else(|| file.and_then(|f| set.find_syntax_by_extension(f)))
    });
    let first = text.lines().next().unwrap_or("");
    by_name.or_else(|| set.find_syntax_by_first_line(first)).filter(|s| !is_plain(s)).map(|s| s.name.clone())
}

fn is_plain(syntax: &SyntaxReference) -> bool {
    syntax.name == "Plain Text"
}

/// The languages to choose from, by name, Plain Text left out.
pub fn languages() -> Vec<&'static str> {
    let mut names: Vec<&str> =
        syntaxes().syntaxes().iter().filter(|s| !is_plain(s) && !s.hidden).map(|s| s.name.as_str()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

type State = (ParseState, HighlightState);

/// A run of one style within a line, from the line's start; no colour is
/// the skin's text colour.
#[derive(Clone, Debug, PartialEq)]
struct Span {
    range: Range<u32>,
    color: Option<Color32>,
    italic: bool,
    underline: bool,
}

#[derive(Clone)]
struct Line {
    /// Where the line starts in the text, and its length with its newline.
    start: usize,
    len: usize,
    /// The parser's state at the line's start, as last coloured.
    state: Option<State>,
    spans: Vec<Span>,
    /// Coloured from `state` as the line now reads. Two fresh lines in a
    /// row always agree: the second's state is where the first ended.
    fresh: bool,
}

impl Line {
    fn new(start: usize, len: usize) -> Self {
        Line { start, len, state: None, spans: Vec::new(), fresh: false }
    }
}

pub struct Highlighter {
    /// The language by name, or none for plain text.
    syntax: Option<String>,
    dark: bool,
    text: String,
    lines: Vec<Line>,
    /// Lines before this are coloured right; this one's state is right.
    valid: usize,
    /// Changes whenever what `job` would make does.
    generation: u64,
}

impl Highlighter {
    pub fn new(syntax: Option<String>, dark: bool) -> Self {
        let mut h =
            Highlighter { syntax, dark, text: String::new(), lines: vec![Line::new(0, 0)], valid: 0, generation: 0 };
        h.restart();
        h
    }

    /// A number that changes whenever the colours or text do.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Colours with another language, or the other theme, from the top.
    pub fn set(&mut self, syntax: Option<String>, dark: bool) {
        if syntax != self.syntax || dark != self.dark {
            (self.syntax, self.dark) = (syntax, dark);
            self.restart();
        }
    }

    fn restart(&mut self) {
        for line in &mut self.lines {
            (line.fresh, line.state) = (false, None);
        }
        self.valid = 0;
        self.lines[0].state = self.initial();
        self.generation += 1;
    }

    fn active(&self) -> Option<&'static SyntaxReference> {
        if self.text.len() > MOST_BYTES {
            return None;
        }
        syntaxes().find_syntax_by_name(self.syntax.as_deref()?)
    }

    fn initial(&self) -> Option<State> {
        let syntax = self.active()?;
        Some((ParseState::new(syntax), HighlightState::new(&Styler::new(theme(self.dark)), ScopeStack::new())))
    }

    /// Takes in the text as it now reads: lines outside the edit keep their
    /// colours, and colouring goes back to the edit's first line.
    pub fn update(&mut self, text: &str) {
        if text == self.text {
            return;
        }
        let was_active = self.active().is_some();
        let (old, new) = (self.text.as_bytes(), text.as_bytes());
        let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
        let room = old.len().min(new.len()) - prefix;
        let suffix = old.iter().rev().zip(new.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
        // The line the edit starts in, and the first after it whose text and
        // the newline before it are untouched.
        let a = self.lines.partition_point(|l| l.start <= prefix) - 1;
        let tail_start = old.len() - suffix;
        let b = self.lines.partition_point(|l| l.start <= tail_start);
        let delta = new.len() as isize - old.len() as isize;
        let from = self.lines[a].start;
        // The edited lines start before `until`: line b's new start, else
        // past the end, so a text ending in a newline has an empty last line.
        let (until, end) = match self.lines.get(b) {
            Some(line) => {
                let at = (line.start as isize + delta) as usize;
                (at, at)
            }
            None => (new.len() + 1, new.len()),
        };
        let mut starts = vec![from];
        starts.extend(text[from..end].match_indices('\n').map(|(i, _)| from + i + 1).filter(|&s| s < until));
        let mut middle: Vec<Line> = starts
            .iter()
            .enumerate()
            .map(|(i, &s)| Line::new(s, starts.get(i + 1).copied().unwrap_or(end) - s))
            .collect();
        middle[0].state = self.lines[a].state.clone();
        let mut tail = self.lines.split_off(b);
        for line in &mut tail {
            line.start = (line.start as isize + delta) as usize;
        }
        self.lines.truncate(a);
        self.lines.extend(middle);
        self.lines.extend(tail);
        self.valid = self.valid.min(a);
        self.text = text.to_string();
        if self.active().is_some() != was_active {
            self.restart();
        }
        self.generation += 1;
    }

    /// Whether there's colouring still to do.
    pub fn pending(&self) -> bool {
        self.active().is_some() && self.valid < self.lines.len()
    }

    /// Colours lines for up to `budget`. Whether it's all done.
    pub fn advance(&mut self, budget: Duration) -> bool {
        let Some(syntax) = self.active() else {
            self.valid = self.lines.len();
            return true;
        };
        let set = syntaxes();
        let styler = Styler::new(theme(self.dark));
        let default = theme(self.dark).settings.foreground;
        let deadline = Instant::now() + budget;
        let n = self.lines.len();
        if self.valid >= n {
            return true;
        }
        let mut carry = self.lines[self.valid]
            .state
            .clone()
            .unwrap_or_else(|| (ParseState::new(syntax), HighlightState::new(&styler, ScopeStack::new())));
        let mut done = 0;
        while self.valid < n {
            let i = self.valid;
            let line = &self.lines[i];
            if line.fresh && line.state.as_ref() == Some(&carry) {
                // Coloured right already, and so is every fresh line after
                // it; the state after them comes from the last of them.
                let mut j = i;
                while j + 1 < n && self.lines[j + 1].fresh {
                    j += 1;
                }
                self.valid = j;
                carry = self.lines[j].state.clone().expect("fresh lines have a state");
            }
            let i = self.valid;
            let (start, len) = (self.lines[i].start, self.lines[i].len);
            let before = carry.clone();
            let spans = colour(&self.text[start..start + len], &mut carry, set, &styler, default);
            let line = &mut self.lines[i];
            let changed = !line.fresh || line.spans != spans || line.state.as_ref() != Some(&before);
            (line.state, line.spans, line.fresh) = (Some(before), spans, true);
            if changed {
                self.generation += 1;
            }
            self.valid += 1;
            done += 1;
            if done % 32 == 0 && Instant::now() >= deadline {
                break;
            }
        }
        if self.valid < n {
            // Where colouring picks up next.
            let next = &mut self.lines[self.valid];
            if next.state.as_ref() != Some(&carry) {
                (next.state, next.fresh) = (Some(carry), false);
            }
            return false;
        }
        true
    }

    /// The text as a layout job: coloured lines in their colours, the rest
    /// in `plain`.
    pub fn job(&self, font: &FontId, plain: Color32) -> LayoutJob {
        let mut job = LayoutJob { text: self.text.clone(), ..Default::default() };
        let format = |color: Option<Color32>, italic: bool, underline: bool| {
            let color = color.unwrap_or(plain);
            TextFormat {
                font_id: font.clone(),
                color,
                italics: italic,
                underline: if underline { Stroke::new(1.0, color) } else { Stroke::NONE },
                ..Default::default()
            }
        };
        let colouring = self.active().is_some();
        let mut plain_from: Option<usize> = None;
        for line in &self.lines {
            let covers = line.spans.last().is_some_and(|s| s.range.end as usize == line.len);
            if !colouring || !covers {
                plain_from.get_or_insert(line.start);
                continue;
            }
            if let Some(from) = plain_from.take()
                && from < line.start
            {
                job.sections.push(section(from..line.start, format(None, false, false)));
            }
            for span in &line.spans {
                let range = line.start + span.range.start as usize..line.start + span.range.end as usize;
                job.sections.push(section(range, format(span.color, span.italic, span.underline)));
            }
        }
        if let Some(from) = plain_from
            && from < self.text.len()
        {
            job.sections.push(section(from..self.text.len(), format(None, false, false)));
        }
        if job.sections.is_empty() {
            job.sections.push(section(0..self.text.len(), format(None, false, false)));
        }
        job
    }
}

fn section(range: Range<usize>, format: TextFormat) -> LayoutSection {
    LayoutSection { leading_space: 0.0, byte_range: ByteIndex(range.start)..ByteIndex(range.end), format }
}

/// One line's spans, moving `state` past it.
fn colour(
    line: &str,
    state: &mut State,
    set: &SyntaxSet,
    styler: &Styler,
    default: Option<syntect::highlighting::Color>,
) -> Vec<Span> {
    let whole = || vec![Span { range: 0..line.len() as u32, color: None, italic: false, underline: false }];
    if line.is_empty() {
        return Vec::new();
    }
    if line.len() > LONGEST_LINE {
        return whole();
    }
    let Ok(ops) = state.0.parse_line(line, set) else { return whole() };
    let mut spans: Vec<Span> = Vec::new();
    for (style, _, range) in RangedHighlightIterator::new(&mut state.1, &ops, line, styler) {
        let fg = style.foreground;
        let color = (Some(fg) != default).then(|| Color32::from_rgb(fg.r, fg.g, fg.b));
        let span = Span {
            range: range.start as u32..range.end as u32,
            color,
            italic: style.font_style.contains(FontStyle::ITALIC),
            underline: style.font_style.contains(FontStyle::UNDERLINE),
        };
        // Runs of one style are one span.
        match spans.last_mut() {
            Some(last)
                if last.range.end == span.range.start
                    && (&last.color, last.italic, last.underline) == (&span.color, span.italic, span.underline) =>
            {
                last.range.end = span.range.end
            }
            _ => spans.push(span),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(h: &mut Highlighter) {
        while !h.advance(Duration::from_secs(5)) {}
    }

    /// Colours `text` from nothing, for comparing with what edits made.
    fn fresh(text: &str) -> Highlighter {
        let mut h = Highlighter::new(Some("Rust".into()), true);
        h.update(text);
        done(&mut h);
        h
    }

    fn sections(h: &Highlighter) -> Vec<(Range<usize>, Color32)> {
        h.job(&FontId::monospace(12.0), Color32::WHITE)
            .sections
            .iter()
            .map(|s| (s.byte_range.start.0..s.byte_range.end.0, s.format.color))
            .collect()
    }

    fn starts(h: &Highlighter) -> Vec<usize> {
        h.lines.iter().map(|l| l.start).collect()
    }

    #[test]
    fn finds_languages_by_name_and_first_line() {
        assert_eq!(detect(Some("main.rs"), "").as_deref(), Some("Rust"));
        assert_eq!(detect(Some("Makefile"), "").as_deref(), Some("Makefile"));
        assert_eq!(detect(Some("Cargo.toml"), "").as_deref(), Some("TOML"));
        assert_eq!(detect(None, "#!/bin/sh\necho hi\n").as_deref(), Some("Bourne Again Shell (bash)"));
        assert_eq!(detect(Some("notes.txt"), "hello"), None);
        assert!(languages().contains(&"Rust"));
    }

    #[test]
    fn lines_follow_the_text() {
        let mut h = Highlighter::new(None, true);
        h.update("a\nbb\n");
        assert_eq!(starts(&h), [0, 2, 5]);
        h.update("a\nbXb\nc");
        assert_eq!(starts(&h), [0, 2, 6]);
        h.update("");
        assert_eq!(starts(&h), [0]);
        h.update("\n\n");
        assert_eq!(starts(&h), [0, 1, 2]);
    }

    #[test]
    fn edits_colour_as_a_fresh_start_does() {
        let edits = [
            "fn main() {\n    let x = 1;\n}\n",
            "fn main() {\n    let x = \"a\";\n}\n",
            "fn main() {\n    /* let x = \"a\";\n}\n",
            "fn main() {\n    /* let x = \"a\"; */\n}\n",
            "// top\nfn main() {\n    /* let x = \"a\"; */\n}\n",
            "// top\nfn main() {\n}\n",
            "",
            "struct A;\n",
        ];
        let mut h = Highlighter::new(Some("Rust".into()), true);
        for text in edits {
            h.update(text);
            done(&mut h);
            assert_eq!(starts(&h), starts(&fresh(text)), "{text:?}");
            assert_eq!(sections(&h), sections(&fresh(text)), "{text:?}");
        }
    }

    #[test]
    fn an_edit_recolours_little() {
        let text: String = (0..2000).map(|i| format!("let v{i} = {i};\n")).collect();
        let mut h = fresh(&text);
        let edited = text.replacen("let v5 =", "let v5x =", 1);
        h.update(&edited);
        assert!(h.valid <= 5);
        // One line's worth, then the rest are known to be right.
        assert!(h.advance(Duration::from_secs(5)));
        assert_eq!(sections(&h), sections(&fresh(&edited)));
    }

    #[test]
    fn colouring_stopped_part_way_carries_on_right() {
        let text: String = (0..3000).map(|i| format!("let s{i} = \"{i}\";\n")).collect();
        let mut h = fresh(&text);
        // Opening a comment changes every line after it.
        let edited = format!("/*\n{text}");
        h.update(&edited);
        let mut rounds = 0;
        while !h.advance(Duration::ZERO) {
            rounds += 1;
        }
        assert!(rounds > 1);
        assert_eq!(sections(&h), sections(&fresh(&edited)));
    }

    #[test]
    fn toml_is_coloured() {
        let mut h = Highlighter::new(Some("TOML".into()), true);
        h.update("[package]\nname = \"text-editor\"\n");
        done(&mut h);
        let colours: std::collections::HashSet<Color32> = sections(&h).into_iter().map(|(_, c)| c).collect();
        assert!(colours.len() >= 3, "{colours:?}");
    }

    #[test]
    fn plain_text_is_the_skins_colour() {
        let mut h = Highlighter::new(None, true);
        h.update("hello\n");
        assert_eq!(sections(&h), [(0..6, Color32::WHITE)]);
        assert!(!h.pending());
    }
}
