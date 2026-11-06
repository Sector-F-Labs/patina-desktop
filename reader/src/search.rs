//! Finding text on screen, case aside: the byte ranges where a query
//! appears, for marking (man pages and source; markdown searches itself in
//! Patina's editor).

use std::ops::Range;

/// A character as search compares it: lower-cased one for one, so the
/// folded text lines up with the text.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Every place `query` appears in `text`, case aside, as byte ranges, not
/// overlapping.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let hay: Vec<(usize, char)> = text.char_indices().map(|(i, c)| (i, fold(c))).collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at + needle.len() <= hay.len() {
        if hay[at..at + needle.len()].iter().map(|(_, c)| *c).eq(needle.iter().copied()) {
            let end = hay.get(at + needle.len()).map_or(text.len(), |(i, _)| *i);
            found.push(hay[at].0..end);
            at += needle.len();
        } else {
            at += 1;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_place_case_aside_as_byte_ranges() {
        assert_eq!(find_all("Fn main() { fn x }", "FN"), vec![0..2, 12..14]);
        assert_eq!(find_all("café Café", "café"), vec![0..5, 6..11]);
        assert_eq!(find_all("aaaa", "aa"), vec![0..2, 2..4]);
        assert!(find_all("text", "").is_empty());
        assert!(find_all("text", "nope").is_empty());
    }
}
