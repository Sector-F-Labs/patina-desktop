//! Man pages: a page's source (`foo.1`, `foo.8.gz`) formatted by the
//! system's man tools, as a terminal shows it, with bold and underline. The
//! formatter marks those with backspace overstrikes (`b\bb` bold, `_\bx`
//! underlined), which are parsed into styles here and drawn in the skin's
//! colours. mandoc is used where there is one (FreeBSD's base, and Ubuntu
//! with it installed), else man-db's `man -l`.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

use egui::text::{LayoutJob, TextFormat};
use egui::{Id, Stroke};

/// Columns a page is formatted to, as in a standard terminal.
const WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Underline,
}

/// A formatted page: its text, and the byte ranges of it that are styled.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
}

/// Whether `path` names a man page's source: a section number (`1` to
/// `9`, perhaps with letters, `3p`) as its extension, perhaps gzipped.
pub fn is_page(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let name = name.strip_suffix(".gz").unwrap_or(name);
    let Some((stem, section)) = name.rsplit_once('.') else { return false };
    let mut chars = section.chars();
    !stem.is_empty()
        && section.len() <= 3
        && chars.next().is_some_and(|c| ('1'..='9').contains(&c))
        && chars.all(|c| c.is_ascii_alphabetic())
}

/// The installed page for `name` (`ls`, `printf(3)`, `3 printf`), through
/// `man -w`.
pub fn find(name: &str) -> Option<PathBuf> {
    let name = name.trim();
    // "printf(3)" as man takes it: "3 printf".
    let args: Vec<String> = match name.strip_suffix(')').and_then(|n| n.split_once('(')) {
        Some((page, section)) => vec![section.into(), page.into()],
        None => name.split_whitespace().map(str::to_string).collect(),
    };
    if args.is_empty() {
        return None;
    }
    let out = Command::new("man").arg("-w").args(&args).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// Formats the page at `path`.
pub fn render(path: &Path) -> Result<Page, String> {
    let mandoc = Command::new("mandoc").args(["-T", "utf8", "-O", &format!("width={WIDTH}")]).arg(path).output();
    let output = match mandoc {
        Ok(out) if out.status.success() => out,
        _ => Command::new("man")
            .arg("-l")
            .arg(path)
            .env("MANWIDTH", WIDTH.to_string())
            .env("MAN_KEEP_FORMATTING", "1")
            .env("GROFF_NO_SGR", "1")
            .env("MANPAGER", "cat")
            .env("PAGER", "cat")
            .output()
            .map_err(|e| format!("can't run man: {e}"))?,
    };
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse(&String::from_utf8_lossy(&output.stdout)))
}

/// Text with overstrikes, as styled text: `x\bx` is a bold x, `_\bx` an
/// underlined one; any other pair keeps the second character.
pub fn parse(raw: &str) -> Page {
    let chars: Vec<char> = raw.chars().collect();
    let mut page = Page::default();
    let mut i = 0;
    while i < chars.len() {
        let (shown, style) = if i + 2 < chars.len() && chars[i + 1] == '\u{8}' {
            let (first, second) = (chars[i], chars[i + 2]);
            i += 3;
            // Bold underlined text repeats the overstrike; take what's left.
            while i + 1 < chars.len() && chars[i] == '\u{8}' {
                i += 2;
            }
            let style = if first == second {
                Some(Style::Bold)
            } else if first == '_' {
                Some(Style::Underline)
            } else {
                None
            };
            (second, style)
        } else {
            i += 1;
            (chars[i - 1], None)
        };
        let start = page.text.len();
        page.text.push(shown);
        let end = page.text.len();
        if let Some(style) = style {
            match page.styles.last_mut() {
                Some((range, last)) if *last == style && range.end == start => range.end = end,
                _ => page.styles.push((start..end, style)),
            }
        }
    }
    page
}

/// Shows `page` in a monospace font: bold in the strong text colour,
/// underlined in the hyperlink colour, selectable and copyable.
pub fn show(ui: &mut egui::Ui, page: &Page, id: Id) {
    let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
        let text = text.as_str();
        let v = ui.visuals();
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let plain = TextFormat { font_id: font.clone(), color: v.text_color(), ..Default::default() };
        let mut job = LayoutJob::default();
        let mut at = 0;
        for (range, style) in &page.styles {
            if range.start < at || range.end > text.len() {
                continue;
            }
            job.append(&text[at..range.start], 0.0, plain.clone());
            let format = match style {
                Style::Bold => TextFormat { color: v.strong_text_color(), ..plain.clone() },
                Style::Underline => TextFormat {
                    color: v.hyperlink_color,
                    underline: Stroke::new(1.0, v.hyperlink_color),
                    ..plain.clone()
                },
            };
            job.append(&text[range.clone()], 0.0, format);
            at = range.end;
        }
        job.append(&text[at.min(text.len())..], 0.0, plain);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let mut shown = page.text.as_str();
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(f32::INFINITY)
            .layouter(&mut layouter),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overstrikes_become_styles() {
        let page = parse("N\u{8}NA\u{8}A x _\u{8}i_\u{8}t +\u{8}o");
        assert_eq!(page.text, "NA x it o");
        assert_eq!(page.styles, vec![(0..2, Style::Bold), (5..7, Style::Underline)]);
        // Bold and underlined: the overstrike repeated.
        let page = parse("_\u{8}B\u{8}B!");
        assert_eq!(page.text, "B!");
        assert_eq!(page.styles, vec![(0..1, Style::Underline)]);
    }

    #[test]
    fn styles_are_byte_ranges_of_the_text() {
        let page = parse("é\u{8}éa");
        assert_eq!(page.text, "éa");
        assert_eq!(page.styles, vec![(0..2, Style::Bold)]);
    }

    #[test]
    fn man_page_names() {
        for yes in ["ls.1", "wbindkeys.1", "printf.3p", "x.8.gz"] {
            assert!(is_page(Path::new(yes)), "{yes}");
        }
        for no in ["main.rs", "README.md", "v1.0", ".1", "a.10", "x.1.tar"] {
            assert!(!is_page(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn formats_a_page() {
        let dir = std::env::temp_dir().join(format!("reader-man-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("t.1");
        std::fs::write(&src, ".TH T 1\n.SH NAME\nt \\- test\n.SH DESCRIPTION\n\\fBbold\\fR and \\fIitalic\\fR.\n")
            .unwrap();
        let page = render(&src).expect("man or mandoc formats it");
        assert!(page.text.contains("NAME") && page.text.contains("bold and italic"), "{}", page.text);
        let styled: Vec<&str> = page.styles.iter().map(|(r, _)| &page.text[r.clone()]).collect();
        assert!(styled.contains(&"bold"), "{styled:?}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
