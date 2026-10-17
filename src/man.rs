//! Man pages: a page's source (`foo.1`, `foo.8.gz`) formatted by the
//! system's man tools, as a terminal shows it, with bold and underline. The
//! formatter marks those with backspace overstrikes (`b\bb` bold, `_\bx`
//! underlined), which are parsed into styles here and drawn in the skin's
//! colours. mandoc is used where there is one (FreeBSD's base, and Ubuntu
//! with it installed), else man-db's `man -l`.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use egui::text::{LayoutJob, TextFormat};
use egui::{Id, Stroke};

/// Columns a page is formatted to, as in a standard terminal.
const WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Bold,
    Underline,
}

/// A formatted page: its text, the byte ranges of it that are styled, and
/// the ones that lead somewhere.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub text: String,
    pub styles: Vec<(Range<usize>, Style)>,
    pub links: Vec<(Range<usize>, Link)>,
}

/// Where a reference in a page leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Another page, `printf(3)`, as man names it.
    Page(String),
    /// A web address.
    Web(String),
    /// A file or folder that's there: examples, configuration, documents.
    Path(PathBuf),
}

/// What showing a page did: a link clicked, and where the current found
/// text is on screen.
#[derive(Default)]
pub struct Shown {
    pub clicked: Option<Link>,
    pub current: Option<egui::Rect>,
}

/// Whether `path` names a man page's source: a section number (`1` to
/// `9`, perhaps with letters, `3p`, `2const`, `3readline`) as its
/// extension, perhaps gzipped.
pub fn is_page(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let name = name.strip_suffix(".gz").unwrap_or(name);
    let Some((stem, section)) = name.rsplit_once('.') else { return false };
    let mut chars = section.chars();
    !stem.is_empty() && chars.next().is_some_and(|c| ('1'..='9').contains(&c)) && chars.all(|c| c.is_ascii_alphabetic())
}

/// The installed page for `name` (`ls`, `printf(3)`, `3 printf`), through
/// `man -w` on Reader's manpath.
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
    let manpath = std::env::join_paths(folders()).ok()?;
    let out = Command::new("man").arg("-w").args(&args).env("MANPATH", manpath).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
    (out.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

/// The folders man pages are installed in, from `manpath`, else the usual
/// ones, then the per-user ones `manpath` leaves out when their `bin` isn't
/// on the PATH, as when Reader starts from the desktop rather than a shell.
pub fn manpath() -> Vec<PathBuf> {
    let found = Command::new("manpath")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|p| !p.is_empty());
    let path = found.unwrap_or_else(|| "/usr/local/share/man:/usr/local/man:/usr/share/man".into());
    let mut folders: Vec<PathBuf> = path.split(':').map(PathBuf::from).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data =
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
    for user in [data.map(|d| d.join("man")), home.map(|h| h.join(".cargo/share/man"))].into_iter().flatten() {
        if !folders.contains(&user) {
            folders.push(user);
        }
    }
    folders.retain(|p| p.is_dir());
    folders
}

/// The manpath as it was first asked for, for saying where pages come from
/// without running `manpath` each frame.
pub fn folders() -> &'static [PathBuf] {
    static FOLDERS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    FOLDERS.get_or_init(manpath)
}

/// What each section holds, as man(1) names them.
pub fn section_title(section: char) -> &'static str {
    match section {
        '1' => "User Commands",
        '2' => "System Calls",
        '3' => "Library Functions",
        '4' => "Devices",
        '5' => "File Formats",
        '6' => "Games",
        '7' => "Miscellaneous",
        '8' => "System Administration",
        '9' => "Kernel",
        _ => "Other",
    }
}

/// A page file's name without its section and compression: `ls.1.gz` is
/// `ls`, `printf.3p` is `printf`.
pub fn page_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = name.strip_suffix(".gz").unwrap_or(&name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem).to_string()
}

/// A page as man names it: `ls(1)`, `printf(3p)`.
pub fn label(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = name.strip_suffix(".gz").unwrap_or(&name);
    match name.rsplit_once('.') {
        Some((page, section)) => format!("{page}({section})"),
        None => name.to_string(),
    }
}

/// The tree's folder for the page's section (see `tree::man_pages`).
pub fn section_folder(path: &Path) -> Option<PathBuf> {
    let label = label(path);
    let section = label.rsplit_once('(')?.1.chars().next()?;
    Some(PathBuf::from(format!("man:{section}")))
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
    let mut page = parse(&String::from_utf8_lossy(&output.stdout));
    page.links = links(&page.text);
    Ok(page)
}

/// The references in a page's text: other pages (`ls(1)`, `printf(3p)`),
/// web addresses, and absolute paths to files or folders that exist (the
/// examples under `/usr/share/doc`, say).
pub fn links(text: &str) -> Vec<(Range<usize>, Link)> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &text[i..];
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let end = i + token_end(rest);
            found.push((i..end, Link::Web(text[i..end].to_string())));
            i = end;
            continue;
        }
        let starts_word = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || b"/._-~".contains(&bytes[i - 1]));
        if bytes[i] == b'/' && starts_word {
            let end = i + token_end(rest);
            let path = PathBuf::from(&text[i..end]);
            if end > i + 1 && text[i + 1..end].contains('/') && path.exists() {
                found.push((i..end, Link::Path(path)));
                i = end;
                continue;
            }
        }
        if bytes[i] == b'('
            && let Some((start, end, page)) = page_ref(text, i)
        {
            found.push((start..end, Link::Page(page)));
            i = end;
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// Where a URL or path starting `rest` ends: at a space or a closing
/// bracket or quote, without trailing punctuation.
fn token_end(rest: &str) -> usize {
    let end = rest.find(|c: char| c.is_whitespace() || "<>()[]\"'`".contains(c)).unwrap_or(rest.len());
    rest[..end].trim_end_matches(['.', ',', ';', ':']).len()
}

/// A page reference whose `(` is at `open`: `name(section)`, with the
/// name's start and the reference's end.
fn page_ref(text: &str, open: usize) -> Option<(usize, usize, String)> {
    let close = open + text[open..].find(')')?;
    let section = &text[open + 1..close];
    let mut chars = section.chars();
    let valid = chars.next().is_some_and(|c| ('1'..='9').contains(&c))
        && section.len() <= 10
        && chars.all(|c| c.is_ascii_lowercase());
    if !valid {
        return None;
    }
    let name_part = |c: char| c.is_ascii_alphanumeric() || "_.:+-".contains(c);
    let before = &text[..open];
    let start = before.char_indices().rev().take_while(|(_, c)| name_part(*c)).last().map_or(open, |(i, _)| i);
    let name = before[start..].trim_start_matches(['.', '-', ':']);
    let start = open - name.len();
    (!name.is_empty() && name.starts_with(|c: char| c.is_ascii_alphanumeric()))
        .then(|| (start, close + 1, format!("{name}({section})")))
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
/// underlined text underlined, references in the link colour and
/// clickable; selectable and copyable. `marks` are found text (byte ranges),
/// the `current` one marked more strongly.
pub fn show(ui: &mut egui::Ui, page: &Page, id: Id, marks: &[Range<usize>], current: Option<usize>) -> Shown {
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
                Style::Underline => TextFormat { underline: Stroke::new(1.0, v.text_color()), ..plain.clone() },
            };
            job.append(&text[range.clone()], 0.0, format);
            at = range.end;
        }
        job.append(&text[at.min(text.len())..], 0.0, plain);
        // References over whatever style they had: the link colour, underlined.
        let link = TextFormat {
            color: v.hyperlink_color,
            underline: Stroke::new(1.0, v.hyperlink_color),
            ..TextFormat { font_id: font, ..Default::default() }
        };
        recolour(&mut job, &page.links, &link);
        let selection = v.selection.bg_fill;
        patina::markdown::embeds::mark(&mut job, marks, current, selection.gamma_multiply(0.35), selection);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let mut shown = page.text.as_str();
    let output = egui::TextEdit::multiline(&mut shown)
        .id(id)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .desired_width(f32::INFINITY)
        .layouter(&mut layouter)
        .show(ui);
    let link_at = |char_index: usize| {
        let byte = page.text.char_indices().nth(char_index).map_or(page.text.len(), |(b, _)| b);
        page.links.iter().find(|(r, _)| r.contains(&byte)).map(|(_, l)| l.clone())
    };
    let mut out = Shown::default();
    if let Some(pointer) = output.response.hover_pos() {
        let at = output.galley.cursor_from_pos(pointer - output.galley_pos).index.0;
        if link_at(at).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    // A click, not a drag: the cursor lands on what was clicked.
    if output.response.clicked()
        && let Some(range) = output.cursor_range
        && range.is_empty()
    {
        out.clicked = link_at(range.primary.index.0);
    }
    out.current = current.and_then(|i| marks.get(i)).map(|m| {
        let chars = page.text[..m.start].chars().count();
        output.galley.pos_from_cursor(egui::text::CCursor::new(chars)).translate(output.galley_pos.to_vec2())
    });
    out
}

/// Gives the parts of `job` in `ranges` the `format`'s colour and underline,
/// splitting sections where they start and end.
fn recolour(job: &mut LayoutJob, ranges: &[(Range<usize>, Link)], format: &TextFormat) {
    if ranges.is_empty() {
        return;
    }
    let mut cuts: Vec<usize> = ranges.iter().flat_map(|(r, _)| [r.start, r.end]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut sections = Vec::with_capacity(job.sections.len() + cuts.len());
    for section in std::mem::take(&mut job.sections) {
        let (first, last) = (section.byte_range.start.0, section.byte_range.end.0);
        let mut start = first;
        for end in cuts.iter().copied().filter(|&c| c > first && c < last).chain(std::iter::once(last)) {
            let mut piece = section.clone();
            piece.byte_range = egui::text::ByteIndex(start)..egui::text::ByteIndex(end);
            if start > first {
                piece.leading_space = 0.0;
            }
            if ranges.iter().any(|(r, _)| r.start <= start && end <= r.end) {
                piece.format.color = format.color;
                piece.format.underline = format.underline;
            }
            sections.push(piece);
            start = end;
        }
    }
    job.sections = sections;
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
    fn references_become_links() {
        let text = "See ls(1), printf(3p) and https://example.org/x. Not (1) or v2(final).\nExamples in /usr/share/doc, gone /no/such/thing.";
        let found = links(text);
        let shown: Vec<(&str, &Link)> = found.iter().map(|(r, l)| (&text[r.clone()], l)).collect();
        assert_eq!(shown[0], ("ls(1)", &Link::Page("ls(1)".into())));
        assert_eq!(shown[1], ("printf(3p)", &Link::Page("printf(3p)".into())));
        assert_eq!(shown[2], ("https://example.org/x", &Link::Web("https://example.org/x".into())));
        assert_eq!(shown[3], ("/usr/share/doc", &Link::Path("/usr/share/doc".into())));
        assert_eq!(found.len(), 4, "{shown:?}");
    }

    #[test]
    fn styles_are_byte_ranges_of_the_text() {
        let page = parse("é\u{8}éa");
        assert_eq!(page.text, "éa");
        assert_eq!(page.styles, vec![(0..2, Style::Bold)]);
    }

    #[test]
    fn pages_are_labelled_as_man_names_them() {
        assert_eq!(label(Path::new("/m/man1/ls.1.gz")), "ls(1)");
        assert_eq!(label(Path::new("printf.3p")), "printf(3p)");
        assert_eq!(section_folder(Path::new("printf.3p.gz")), Some(PathBuf::from("man:3")));
    }

    #[test]
    fn page_names_drop_the_section() {
        assert_eq!(page_name(Path::new("/m/man1/ls.1.gz")), "ls");
        assert_eq!(page_name(Path::new("printf.3p")), "printf");
        assert_eq!(page_name(Path::new("Algorithm::Diff.3pm.gz")), "Algorithm::Diff");
    }

    #[test]
    fn per_user_pages_are_on_the_manpath_whatever_the_path() {
        let cargo = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cargo/share/man");
        if cargo.is_dir() {
            assert!(manpath().contains(&cargo), "{:?}", manpath());
        }
    }

    #[test]
    fn man_page_names() {
        for yes in
            ["ls.1", "wbindkeys.1", "printf.3p", "x.8.gz", "EVP_MD.3ssl.gz", "F_GETFL.2const", "history.3readline"]
        {
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
