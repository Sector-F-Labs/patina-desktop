//! Find and replace: every search is a regex, the plain text escaped,
//! case and whole words as flags, so the three options combine freely.

use std::ops::Range;

use egui::Color32;
use egui::text::{ByteIndex, LayoutJob};
use regex::{Regex, RegexBuilder};

/// More matches than this aren't marked or counted further.
pub const MOST_MATCHES: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub text: String,
    pub case: bool,
    pub whole_word: bool,
    pub regex: bool,
}

impl Query {
    /// The search as a regex; an error for a regex that doesn't parse.
    fn compile(&self) -> Result<Option<Regex>, String> {
        if self.text.is_empty() {
            return Ok(None);
        }
        let pattern = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        let pattern = if self.whole_word { format!(r"\b(?:{pattern})\b") } else { pattern };
        RegexBuilder::new(&pattern).case_insensitive(!self.case).multi_line(true).build().map(Some).map_err(|e| match e
        {
            regex::Error::Syntax(s) => {
                s.lines().last().unwrap_or("not a regex").trim_start_matches("error: ").to_string()
            }
            other => other.to_string(),
        })
    }

    /// Every match in `text` as byte ranges, empty matches left out.
    pub fn find_all(&self, text: &str) -> Result<Vec<Range<usize>>, String> {
        let Some(re) = self.compile()? else { return Ok(Vec::new()) };
        Ok(re.find_iter(text).filter(|m| !m.is_empty()).map(|m| m.range()).take(MOST_MATCHES).collect())
    }

    /// What the match at `at` becomes: `with` as it is, or with `$1` and
    /// `${name}` filled in for a regex.
    pub fn replacement(&self, text: &str, at: &Range<usize>, with: &str) -> Option<String> {
        if !self.regex {
            return Some(with.to_string());
        }
        let re = self.compile().ok()??;
        let caps = re.captures_at(text, at.start).filter(|c| c.get(0).is_some_and(|m| m.range() == *at))?;
        let mut out = String::new();
        caps.expand(with, &mut out);
        Some(out)
    }

    /// `text` with every match replaced, and how many there were.
    pub fn replace_all(&self, text: &str, with: &str) -> Result<(String, usize), String> {
        let Some(re) = self.compile()? else { return Ok((text.to_string(), 0)) };
        let count = re.find_iter(text).filter(|m| !m.is_empty()).count();
        if count == 0 {
            return Ok((text.to_string(), 0));
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for caps in re.captures_iter(text) {
            let m = caps.get(0).expect("group 0 is the match");
            if m.is_empty() {
                continue;
            }
            out.push_str(&text[last..m.start()]);
            if self.regex {
                caps.expand(with, &mut out);
            } else {
                out.push_str(with);
            }
            last = m.end();
        }
        out.push_str(&text[last..]);
        Ok((out, count))
    }
}

/// The match at or after byte `from`, wrapping round; the one before it
/// going back.
pub fn next(found: &[Range<usize>], from: usize, forward: bool) -> Option<usize> {
    if found.is_empty() {
        return None;
    }
    if forward {
        Some(found.partition_point(|m| m.start < from) % found.len())
    } else {
        Some(found.partition_point(|m| m.end <= from).checked_sub(1).unwrap_or(found.len() - 1))
    }
}

/// Marks found text in a layout job, the current match more strongly.
/// `marks` are sorted and apart, as `find_all` gives them; ranges that
/// don't fit the job's text (it changed this frame) are left out.
pub fn mark(job: &mut LayoutJob, marks: &[Range<usize>], current: Option<usize>, colour: Color32, strong: Color32) {
    let text = &job.text;
    let fits = |m: &Range<usize>| m.end <= text.len() && text.is_char_boundary(m.start) && text.is_char_boundary(m.end);
    if marks.is_empty() || !marks.iter().all(fits) {
        return;
    }
    let mut sections = Vec::with_capacity(job.sections.len() + marks.len() * 2);
    let mut m = 0;
    for section in std::mem::take(&mut job.sections) {
        let (first, last) = (section.byte_range.start.0, section.byte_range.end.0);
        let mut at = first;
        while at < last {
            while m < marks.len() && marks[m].end <= at {
                m += 1;
            }
            let (end, fill) = match marks.get(m) {
                Some(mark) if mark.start <= at => {
                    (mark.end.min(last), Some(if current == Some(m) { strong } else { colour }))
                }
                Some(mark) => (mark.start.min(last), None),
                None => (last, None),
            };
            let mut piece = section.clone();
            piece.byte_range = ByteIndex(at)..ByteIndex(end);
            if at > first {
                piece.leading_space = 0.0;
            }
            if let Some(fill) = fill {
                piece.format.background = fill;
            }
            sections.push(piece);
            at = end;
        }
    }
    job.sections = sections;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Query {
        Query { text: text.into(), ..Query::default() }
    }

    #[test]
    fn plain_text_is_literal_and_case_aside_unless_asked() {
        assert_eq!(q("a.b").find_all("a.b axb A.B").unwrap(), [0..3, 8..11]);
        let case = Query { case: true, ..q("a.b") };
        assert_eq!(case.find_all("a.b A.B").unwrap(), vec![0..3]);
        assert!(q("").find_all("text").unwrap().is_empty());
    }

    #[test]
    fn whole_words_and_regexes() {
        let word = Query { whole_word: true, ..q("in") };
        assert_eq!(word.find_all("in inside pin in").unwrap(), [0..2, 14..16]);
        let re = Query { regex: true, ..q(r"^\w+") };
        assert_eq!(re.find_all("one two\nthree").unwrap(), [0..3, 8..13]);
        assert!(Query { regex: true, ..q("(") }.find_all("x").is_err());
        // A regex that matches nothing finds nothing.
        assert!(Query { regex: true, ..q("x*") }.find_all("abc").unwrap().is_empty());
    }

    #[test]
    fn replaces_one_and_all() {
        let re = Query { regex: true, ..q(r"(\w+)@(\w+)") };
        let text = "me@home you@work";
        let found = re.find_all(text).unwrap();
        assert_eq!(re.replacement(text, &found[1], "$2:$1").as_deref(), Some("work:you"));
        assert_eq!(re.replace_all(text, "$2:$1").unwrap(), ("home:me work:you".into(), 2));
        // Plain text keeps a $ as it is.
        assert_eq!(q("a").replace_all("a b A", "$1").unwrap(), ("$1 b $1".into(), 2));
        assert_eq!(q("zzz").replace_all("abc", "x").unwrap(), ("abc".into(), 0));
    }

    #[test]
    fn steps_from_the_cursor_and_wraps() {
        let found = [0..2, 5..7, 10..12];
        assert_eq!(next(&found, 3, true), Some(1));
        assert_eq!(next(&found, 5, true), Some(1));
        assert_eq!(next(&found, 11, true), Some(0));
        assert_eq!(next(&found, 5, false), Some(0));
        assert_eq!(next(&found, 0, false), Some(2));
        assert_eq!(next(&[], 0, true), None);
    }

    #[test]
    fn marks_split_sections() {
        let mut job = LayoutJob::single_section("hello world".into(), Default::default());
        mark(&mut job, &[1..3, 6..11], Some(1), Color32::RED, Color32::BLUE);
        let pieces: Vec<_> =
            job.sections.iter().map(|s| (s.byte_range.start.0..s.byte_range.end.0, s.format.background)).collect();
        assert_eq!(
            pieces,
            [(0..1, Color32::TRANSPARENT), (1..3, Color32::RED), (3..6, Color32::TRANSPARENT), (6..11, Color32::BLUE)]
        );
        // Marks past the text, from before an edit, aren't drawn.
        let mut short = LayoutJob::single_section("hi".into(), Default::default());
        mark(&mut short, &[0..5, 6..7], None, Color32::RED, Color32::BLUE);
        assert_eq!(short.sections.len(), 1);
    }
}
