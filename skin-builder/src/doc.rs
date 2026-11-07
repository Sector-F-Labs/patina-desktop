//! A skin file being edited. It stays a TOML document, so keys this editor has no control for
//! (`[window.controls]`, an app's `[app.*]` section) and comments survive an edit.

use egui::{Color32, Visuals};
use toml_edit::{DocumentMut, Item, Table, value};

/// One colour in the file.
pub struct Field {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
}

const fn f(
    section: &'static str,
    key: &'static str,
    label: &'static str,
    hint: &'static str,
) -> Field {
    Field {
        section,
        key,
        label,
        hint,
    }
}

/// The colours of the shared sections (Patina's docs/skinning.md), in the order they're shown.
pub const FIELDS: &[Field] = &[
    f(
        "colors",
        "background",
        "Background",
        "Panels, and the windows inside the app",
    ),
    f(
        "colors",
        "surface",
        "Surface",
        "Inset areas: text fields, lists, slider tracks",
    ),
    f(
        "colors",
        "surface_alt",
        "Surface, alternate",
        "Alternate rows in lists",
    ),
    f(
        "colors",
        "border",
        "Border",
        "Outlines of windows and panels",
    ),
    f("colors", "text", "Text", "Normal text"),
    f(
        "colors",
        "text_strong",
        "Text, strong",
        "Headings, and text on hovered controls",
    ),
    f(
        "colors",
        "text_weak",
        "Text, weak",
        "Secondary labels, counts, status",
    ),
    f(
        "colors",
        "accent",
        "Accent",
        "Selection, slider fill, toggles that are on",
    ),
    f(
        "colors",
        "accent_text",
        "Accent text",
        "Text drawn on top of the accent",
    ),
    f(
        "colors",
        "link",
        "Link",
        "Links, and the current item in lists",
    ),
    f("colors", "warning", "Warning", "Warnings"),
    f("colors", "error", "Error", "Error messages"),
    f(
        "controls",
        "background",
        "Control",
        "A button or other control at rest",
    ),
    f("controls", "hover", "Control, hovered", "Under the mouse"),
    f("controls", "pressed", "Control, pressed", "While pressed"),
    f(
        "controls",
        "text",
        "Control text",
        "Text and icons on controls",
    ),
    f(
        "controls",
        "text_hover",
        "Control text, hovered",
        "Text on hovered or pressed controls",
    ),
    f("controls", "border", "Control border", "Control outlines"),
    f("window", "title_bar", "Title bar", "The window's title bar"),
    f("window", "title_text", "Title text", "The window's title"),
    f(
        "window",
        "border",
        "Window edge",
        "The one-pixel window edge",
    ),
];

/// The sections' names, as the file has them, with what they're called here.
pub const SECTIONS: &[(&str, &str)] = &[
    ("colors", "Colours"),
    ("controls", "Controls"),
    ("window", "Window"),
];

/// egui's visuals for a skin's `base`.
pub fn visuals(dark: bool) -> Visuals {
    if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    }
}

/// The colour a skin that leaves `field` out gets: egui's own for its base.
pub fn fallback(field: &Field, v: &Visuals) -> Color32 {
    let w = &v.widgets;
    match (field.section, field.key) {
        ("colors", "background") => v.panel_fill,
        ("colors", "surface") => v.extreme_bg_color,
        ("colors", "surface_alt") => v.faint_bg_color,
        ("colors", "border") => w.noninteractive.bg_stroke.color,
        ("colors", "text") => w.noninteractive.fg_stroke.color,
        ("colors", "text_strong") => v.strong_text_color(),
        ("colors", "text_weak") => v.weak_text_color(),
        ("colors", "accent") => v.selection.bg_fill,
        ("colors", "accent_text") => v.selection.stroke.color,
        ("colors", "link") => v.hyperlink_color,
        ("colors", "warning") => v.warn_fg_color,
        ("colors", "error") => v.error_fg_color,
        ("controls", "background") => w.inactive.bg_fill,
        ("controls", "hover") => w.hovered.bg_fill,
        ("controls", "pressed") => w.active.bg_fill,
        ("controls", "text") => w.inactive.fg_stroke.color,
        ("controls", "text_hover") => w.hovered.fg_stroke.color,
        ("controls", "border") => w.inactive.bg_stroke.color,
        ("window", "title_bar") => v.panel_fill,
        ("window", "title_text") => w.noninteractive.fg_stroke.color,
        ("window", "border") => w.noninteractive.bg_stroke.color,
        _ => Color32::GRAY,
    }
}

/// `#RRGGBB`, or `#RRGGBBAA` for one that isn't opaque.
pub fn hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

/// The colour `#RRGGBB` or `#RRGGBBAA` writes.
pub fn parse_hex(text: &str) -> Option<Color32> {
    let digits = text.trim().strip_prefix('#')?;
    if !digits.is_ascii() || !(digits.len() == 6 || digits.len() == 8) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    let (r, g, b) = (byte(0)?, byte(2)?, byte(4)?);
    let a = if digits.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

pub struct SkinDoc {
    doc: DocumentMut,
}

impl SkinDoc {
    /// A new skin: every colour set to what egui has for `dark` or light, to change from there.
    pub fn new(name: &str, dark: bool) -> Self {
        let mut doc = Self {
            doc: DocumentMut::new(),
        };
        doc.doc["format"] = value(1);
        doc.set_name(name);
        doc.set_dark(dark);
        doc.set_corner_radius(Some(6));
        let v = visuals(dark);
        for field in FIELDS {
            doc.set_colour(field, fallback(field, &v));
        }
        doc
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
        Ok(Self { doc })
    }

    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    pub fn name(&self) -> String {
        self.doc
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    pub fn set_name(&mut self, name: &str) {
        self.doc["name"] = value(name);
    }

    pub fn author(&self) -> String {
        self.doc
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    }

    pub fn set_author(&mut self, author: &str) {
        if author.is_empty() {
            self.doc.remove("author");
        } else {
            self.doc["author"] = value(author);
        }
    }

    /// Whether the skin's `base` is dark; a skin without one is taken as dark.
    pub fn dark(&self) -> bool {
        self.doc.get("base").and_then(|v| v.as_str()) != Some("light")
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.doc["base"] = value(if dark { "dark" } else { "light" });
    }

    pub fn corner_radius(&self) -> Option<u8> {
        self.doc
            .get("corner_radius")
            .and_then(|v| v.as_integer())
            .map(|r| r.clamp(0, 255) as u8)
    }

    pub fn set_corner_radius(&mut self, radius: Option<u8>) {
        match radius {
            Some(r) => self.doc["corner_radius"] = value(i64::from(r)),
            None => {
                self.doc.remove("corner_radius");
            }
        }
    }

    /// The window's own corners, `[window] corner_radius`; left out, they follow `corner_radius`, else 15.
    pub fn window_radius(&self) -> Option<u8> {
        self.doc
            .get("window")?
            .get("corner_radius")?
            .as_integer()
            .map(|r| r.clamp(0, 255) as u8)
    }

    pub fn set_window_radius(&mut self, radius: Option<u8>) {
        if let Some(table) = self.section("window") {
            match radius {
                Some(r) => table["corner_radius"] = value(i64::from(r)),
                None => {
                    table.remove("corner_radius");
                }
            }
        }
    }

    pub fn shadows(&self) -> bool {
        self.doc
            .get("shadows")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
    }

    pub fn set_shadows(&mut self, on: bool) {
        self.doc["shadows"] = value(on);
    }

    /// The colour `field` has in the file, if it has one.
    pub fn colour(&self, field: &Field) -> Option<Color32> {
        self.doc
            .get(field.section)?
            .get(field.key)?
            .as_str()
            .and_then(parse_hex)
    }

    pub fn set_colour(&mut self, field: &Field, colour: Color32) {
        if let Some(table) = self.section(field.section) {
            table[field.key] = value(hex(colour));
        }
    }

    /// Takes `field` out, back to what egui has.
    pub fn clear_colour(&mut self, field: &Field) {
        if let Some(table) = self.section(field.section) {
            table.remove(field.key);
        }
    }

    fn section(&mut self, name: &str) -> Option<&mut Table> {
        self.doc
            .entry(name)
            .or_insert(Item::Table(Table::new()))
            .as_table_mut()
    }
}

/// A file name for a skin: its name in lower case, with dashes between words.
pub fn file_name(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let stem = if words.is_empty() {
        "skin".to_string()
    } else {
        words.join("-")
    };
    format!("{stem}.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina::skin::Skin;

    #[test]
    fn colours_round_trip_through_hex() {
        assert_eq!(hex(Color32::from_rgb(0x2d, 0x35, 0x3b)), "#2D353B");
        assert_eq!(
            parse_hex("#2d353b"),
            Some(Color32::from_rgb(0x2d, 0x35, 0x3b))
        );
        let see_through = Color32::from_rgba_unmultiplied(10, 20, 30, 128);
        assert_eq!(hex(see_through), "#0A141E80");
        assert_eq!(parse_hex("#0A141E80"), Some(see_through));
        assert_eq!(parse_hex("2d353b"), None);
        assert_eq!(parse_hex("#2d35"), None);
        assert_eq!(parse_hex("#gggggg"), None);
    }

    #[test]
    fn a_new_skin_is_one_patina_reads() {
        for dark in [true, false] {
            let doc = SkinDoc::new("Test Skin", dark);
            let skin = Skin::parse(&doc.text()).expect("parses");
            assert_eq!(skin.name, "Test Skin");
            assert!(skin.warnings.is_empty(), "{:?}", skin.warnings);
            assert!(skin.colors.accent.is_some());
            assert!(skin.window.title_bar.is_some());
        }
    }

    #[test]
    fn editing_keeps_what_the_editor_doesnt_know() {
        let text = "format = 1\nname = \"X\"\nbase = \"dark\"\n\n[colors]\n# the palette\naccent = \"#112233\"\n\n[window.controls]\nicon = \"#445566\"\n\n[app.unamp]\ntime = \"#778899\"\n";
        let mut doc = SkinDoc::parse(text).unwrap();
        let accent = FIELDS.iter().find(|f| f.key == "accent").unwrap();
        assert_eq!(
            doc.colour(accent),
            Some(Color32::from_rgb(0x11, 0x22, 0x33))
        );
        doc.set_colour(accent, Color32::from_rgb(0xaa, 0xbb, 0xcc));
        let out = doc.text();
        assert!(out.contains("accent = \"#AABBCC\""));
        assert!(out.contains("# the palette"));
        assert!(out.contains("[window.controls]") && out.contains("[app.unamp]"));
        doc.clear_colour(accent);
        assert_eq!(doc.colour(accent), None);
    }

    #[test]
    fn a_skin_with_only_the_required_keys_reads() {
        let doc = SkinDoc::parse("format = 1\nname = \"Bare\"\n").unwrap();
        assert_eq!(doc.name(), "Bare");
        assert_eq!(doc.author(), "");
        assert!(doc.dark() && doc.shadows());
        assert_eq!(doc.corner_radius(), None);
        assert_eq!(doc.colour(&FIELDS[0]), None);
        let none = SkinDoc::parse("").unwrap();
        assert_eq!(none.name(), "");
    }

    #[test]
    fn the_window_has_a_radius_of_its_own() {
        let mut doc = SkinDoc::new("R", true);
        assert_eq!(doc.window_radius(), None);
        doc.set_window_radius(Some(12));
        assert_eq!(doc.window_radius(), Some(12));
        assert_eq!(doc.corner_radius(), Some(6), "the other is separate");
        let skin = Skin::parse(&doc.text()).unwrap();
        assert_eq!(skin.window.corner_radius, Some(12));
        doc.set_window_radius(None);
        assert_eq!(doc.window_radius(), None);
    }

    #[test]
    fn files_are_named_after_the_skin() {
        assert_eq!(file_name("Everforest Dark"), "everforest-dark.toml");
        assert_eq!(file_name("  "), "skin.toml");
        assert_eq!(file_name("../Evil/name"), "evil-name.toml");
    }
}
