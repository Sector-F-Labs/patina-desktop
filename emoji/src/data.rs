//! The emoji, how a search finds them, and the recent ones kept between runs.

use std::path::PathBuf;

use emojis::{Emoji, Group};

/// How many recent emoji are kept.
pub const RECENT: usize = 32;

/// The categories, in the order shown, with what each is called.
pub const GROUPS: [(Group, &str); 9] = [
    (Group::SmileysAndEmotion, "Smileys & Emotion"),
    (Group::PeopleAndBody, "People & Body"),
    (Group::AnimalsAndNature, "Animals & Nature"),
    (Group::FoodAndDrink, "Food & Drink"),
    (Group::TravelAndPlaces, "Travel & Places"),
    (Group::Activities, "Activities"),
    (Group::Objects, "Objects"),
    (Group::Symbols, "Symbols"),
    (Group::Flags, "Flags"),
];

/// The emoji of `group`, without the skin-tone variants of each.
pub fn group(group: Group) -> Vec<&'static Emoji> {
    group
        .emojis()
        .filter(|e| matches!(e.skin_tone(), None | Some(emojis::SkinTone::Default)))
        .collect()
}

/// Every emoji whose name or shortcode matches all the words of `query`,
/// the ones whose name starts with it first, then those with a word that
/// does, then the rest.
pub fn search(query: &str) -> Vec<&'static Emoji> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(u8, &'static Emoji)> = Vec::new();
    for emoji in GROUPS.iter().flat_map(|(g, _)| group(*g)) {
        let name = emoji.name().to_lowercase();
        let codes: Vec<String> = emoji.shortcodes().map(|c| c.replace('_', " ")).collect();
        let hits = |text: &str| words.iter().all(|w| text.contains(w.as_str()));
        if !(hits(&name) || codes.iter().any(|c| hits(c))) {
            continue;
        }
        let rank = if name.starts_with(&words[0]) {
            0
        } else if name.split_whitespace().any(|w| w.starts_with(&words[0])) {
            1
        } else {
            2
        };
        found.push((rank, emoji));
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().map(|(_, e)| e).collect()
}

fn recent_path() -> Option<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(data.join("patina-emoji/recent"))
}

/// The recent emoji, newest first.
pub fn load_recent() -> Vec<&'static Emoji> {
    let text = recent_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    text.lines().filter_map(emojis::get).take(RECENT).collect()
}

/// Puts `emoji` first among the recent ones.
pub fn remember(emoji: &str) {
    let Some(path) = recent_path() else { return };
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let list = bump(&old, emoji);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, list);
}

/// `list` (one emoji a line) with `emoji` moved or added at the top.
fn bump(list: &str, emoji: &str) -> String {
    std::iter::once(emoji)
        .chain(list.lines().filter(|l| *l != emoji && !l.is_empty()))
        .take(RECENT)
        .map(|l| format!("{l}\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_finds_its_emoji_first() {
        let found = search("grinning");
        assert_eq!(found.first().map(|e| e.as_str()), Some("😀"));
        assert!(found.len() > 1, "grinning face with smiling eyes and more");
    }

    #[test]
    fn every_word_has_to_match() {
        let found = search("red heart");
        assert!(found.iter().any(|e| e.as_str() == "❤\u{fe0f}"));
        assert!(search("zzzz nothing").is_empty());
        assert!(search("  ").is_empty());
    }

    #[test]
    fn shortcodes_find_them_too() {
        assert!(search("thumbsup").iter().any(|e| e.as_str() == "👍"));
    }

    #[test]
    fn skin_tone_variants_are_left_out() {
        assert!(
            group(Group::PeopleAndBody).iter().all(
                |e| e.skin_tone().is_none() || e.skin_tone() == Some(emojis::SkinTone::Default)
            )
        );
        assert!(search("waving hand").len() < 6);
    }

    #[test]
    fn the_latest_is_first_and_not_repeated() {
        assert_eq!(bump("", "😀"), "😀\n");
        assert_eq!(bump("😀\n🎉\n", "🎉"), "🎉\n😀\n");
        let many: String = (0..RECENT).map(|i| format!("{i}\n")).collect();
        assert_eq!(bump(&many, "x").lines().count(), RECENT);
    }
}
