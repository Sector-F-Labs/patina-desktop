//! Keyboard layouts (ADR-0068): which the compositor has and which is in
//! use, through its `com.sectorflabs.Patina.Keyboard` on the session bus,
//! for the bar's indicator and Settings; and the layouts xkb offers, from
//! its rules files, for Settings to add from.

use std::path::{Path, PathBuf};

use zbus::blocking::{Connection, Proxy};

/// The most layouts the compositor uses: xkb's rules put no more than four
/// into a keymap, and Wayland's keymap format holds no more (ADR-0068).
pub const MOST_LAYOUTS: usize = 4;

const NAME: &str = "com.sectorflabs.Patina.Keyboard";
const PATH: &str = "/com/sectorflabs/Patina/Keyboard";

/// One of the compositor's layouts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// xkb's layout name: "us".
    pub code: String,
    /// xkb's variant name, or "".
    pub variant: String,
    /// What xkb calls it: "English (US)".
    pub name: String,
    /// What the bar shows for it (see [`label_for`]).
    pub label: String,
}

/// What the bar shows for layout `code`: the code, as "US", when it's a
/// country's or language's two or three letters; else xkb's short
/// description for it ("ᚠ" for a runic one); else its first letters.
pub fn label_for(code: &str) -> String {
    label(code, |code| {
        catalog()
            .iter()
            .find(|e| e.code == code && e.variant.is_empty())
            .map(|e| e.short.clone())
    })
}

fn label(code: &str, short: impl FnOnce(&str) -> Option<String>) -> String {
    if (2..=3).contains(&code.chars().count()) {
        return code.to_uppercase();
    }
    short(code)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| plain_label(code))
}

/// A label for layout `code` in its first letters, as "FUT": for a font
/// without the glyphs of xkb's short description.
pub fn plain_label(code: &str) -> String {
    code.chars().take(3).collect::<String>().to_uppercase()
}

/// The compositor's layouts, in Ctrl+Space's order, and which is in use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub layouts: Vec<Layout>,
    pub current: usize,
}

impl Status {
    pub fn current(&self) -> Option<&Layout> {
        self.layouts.get(self.current)
    }
}

fn session() -> Result<Connection, String> {
    Connection::session().map_err(|e| format!("no session bus: {e}"))
}

fn proxy(bus: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(bus, NAME, PATH, NAME).map_err(|e| e.to_string())
}

/// Whether the compositor serves the layouts here: not under another
/// desktop.
fn served(bus: &Connection) -> Result<bool, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    let name = zbus::names::BusName::try_from(NAME).map_err(|e| e.to_string())?;
    dbus.name_has_owner(name).map_err(|e| e.to_string())
}

fn read(p: &Proxy<'_>) -> Result<Status, String> {
    let layouts: Vec<(String, String, String)> =
        p.get_property("Layouts").map_err(|e| e.to_string())?;
    let current: u32 = p.get_property("Current").map_err(|e| e.to_string())?;
    Ok(Status {
        layouts: layouts
            .into_iter()
            .map(|(code, variant, name)| Layout {
                label: label_for(&code),
                code,
                variant,
                name,
            })
            .collect(),
        current: current as usize,
    })
}

/// The compositor's layouts; `None` when nothing serves them.
pub fn status() -> Result<Option<Status>, String> {
    let bus = session()?;
    if !served(&bus)? {
        return Ok(None);
    }
    read(&proxy(&bus)?).map(Some)
}

/// Switches to layout `index`.
pub fn set_current(index: usize) -> Result<(), String> {
    let bus = session()?;
    proxy(&bus)?
        .call::<_, _, ()>("SetCurrent", &(index as u32,))
        .map_err(|e| e.to_string())
}

/// Gives `send` the layouts now and each time they change, on this thread,
/// until it returns false. While nothing serves them it's given `None`, and
/// asks again every few seconds.
pub fn watch(mut send: impl FnMut(Option<Status>) -> bool) {
    loop {
        if let Ok(bus) = session()
            && served(&bus).unwrap_or(false)
            && let Ok(p) = proxy(&bus)
            && let Ok(changes) = properties(&bus).and_then(|props| {
                props
                    .receive_properties_changed()
                    .map_err(|e| e.to_string())
            })
        {
            if !send(read(&p).ok()) {
                return;
            }
            for _ in changes {
                if !send(read(&p).ok()) {
                    return;
                }
            }
        }
        if !send(None) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

fn properties(bus: &Connection) -> Result<zbus::blocking::fdo::PropertiesProxy<'_>, String> {
    zbus::blocking::fdo::PropertiesProxy::builder(bus)
        .destination(NAME)
        .and_then(|b| b.path(PATH))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

/// The system's layouts and their variants, from `/etc/default/keyboard`
/// (Debian's and Ubuntu's), which the compositor uses when its config has
/// none.
pub fn system_layouts() -> Vec<(String, String)> {
    std::fs::read_to_string("/etc/default/keyboard")
        .map(|text| parse_default_keyboard(&text))
        .unwrap_or_default()
}

/// The `XKBLAYOUT="us,se"` and `XKBVARIANT=",nodeadkeys"` lines, paired.
fn parse_default_keyboard(text: &str) -> Vec<(String, String)> {
    let value = |key: &str| {
        text.lines()
            .filter_map(|l| l.trim().split_once('='))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let (layout, variant) = (value("XKBLAYOUT"), value("XKBVARIANT"));
    let mut variants = variant.split(',');
    layout
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(|code| {
            let variant = variants.next().unwrap_or("").trim();
            (code.to_string(), variant.to_string())
        })
        .collect()
}

/// A layout, or one of its variants, that xkb offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub code: String,
    /// "" for the layout itself.
    pub variant: String,
    /// "Swedish (Dvorak)".
    pub name: String,
    /// xkb's indicator for it: "sv", "ᚠ".
    pub short: String,
}

/// The rules files that list layouts, as xkbcommon finds them: the user's
/// own (`~/.config/xkb`, `~/.xkb`), then the system's, with its extras.
fn registries() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(config) = dirs_config() {
        files.push(config.join("xkb/rules/evdev.xml"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        files.push(Path::new(&home).join(".xkb/rules/evdev.xml"));
    }
    for root in ["/usr/share/X11/xkb", "/usr/local/share/X11/xkb"] {
        files.push(Path::new(root).join("rules/evdev.xml"));
        files.push(Path::new(root).join("rules/evdev.extras.xml"));
    }
    files
}

fn dirs_config() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
}

/// Every layout and variant xkb offers here, sorted by name; the first
/// file to list a layout wins. Read once, the first time it's asked for.
pub fn catalog() -> &'static [Entry] {
    static CATALOG: std::sync::OnceLock<Vec<Entry>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(read_catalog)
}

fn read_catalog() -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for file in registries() {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for entry in parse_registry(&text) {
            if !entries
                .iter()
                .any(|e| e.code == entry.code && e.variant == entry.variant)
            {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    entries
}

/// The layouts and variants in an xkb rules file (evdev.xml).
pub fn parse_registry(text: &str) -> Vec<Entry> {
    let Ok(doc) = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    ) else {
        return Vec::new();
    };
    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        node.children().find(|n| n.has_tag_name(name))
    }
    fn text_of(item: roxmltree::Node<'_, '_>, name: &str) -> String {
        child(item, name)
            .and_then(|n| n.text())
            .unwrap_or_default()
            .trim()
            .to_string()
    }
    let mut entries = Vec::new();
    for layout in doc.descendants().filter(|n| n.has_tag_name("layout")) {
        let Some(item) = child(layout, "configItem") else {
            continue;
        };
        let code = text_of(item, "name");
        if code.is_empty() {
            continue;
        }
        let short = text_of(item, "shortDescription");
        entries.push(Entry {
            code: code.clone(),
            variant: String::new(),
            name: text_of(item, "description"),
            short: short.clone(),
        });
        let variants = child(layout, "variantList")
            .into_iter()
            .flat_map(|list| list.children().filter(|n| n.has_tag_name("variant")));
        for variant in variants {
            let Some(item) = child(variant, "configItem") else {
                continue;
            };
            let own = text_of(item, "shortDescription");
            entries.push(Entry {
                code: code.clone(),
                variant: text_of(item, "name"),
                name: text_of(item, "description"),
                short: if own.is_empty() { short.clone() } else { own },
            });
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE xkbConfigRegistry SYSTEM "xkb.dtd">
<xkbConfigRegistry version="1.1">
  <layoutList>
    <layout>
      <configItem>
        <name>se</name>
        <!-- Keyboard indicator for Swedish layouts -->
        <shortDescription>sv</shortDescription>
        <description>Swedish</description>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>nodeadkeys</name>
            <description>Swedish (no dead keys)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>futhark</name>
        <shortDescription>ᚠ</shortDescription>
        <description>Runic (Younger Futhark, long-branch)</description>
      </configItem>
    </layout>
  </layoutList>
</xkbConfigRegistry>"#;

    #[test]
    fn reads_layouts_and_their_variants() {
        let entries = parse_registry(RULES);
        let names: Vec<_> = entries
            .iter()
            .map(|e| (e.code.as_str(), e.variant.as_str(), e.short.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("se", "", "sv"),
                ("se", "nodeadkeys", "sv"),
                ("futhark", "", "ᚠ")
            ]
        );
        assert_eq!(entries[1].name, "Swedish (no dead keys)");
        assert!(parse_registry("not xml").is_empty());
    }

    #[test]
    fn reads_the_systems_layouts() {
        let text = "XKBMODEL=\"pc105\"\nXKBLAYOUT=\"us,se\"\nXKBVARIANT=\",nodeadkeys\"\n";
        assert_eq!(
            parse_default_keyboard(text),
            [
                ("us".into(), String::new()),
                ("se".into(), "nodeadkeys".into())
            ]
        );
        assert!(parse_default_keyboard("").is_empty());
    }

    #[test]
    fn a_label_is_the_country_or_xkbs_short_name() {
        assert_eq!(label("us", |_| None), "US");
        assert_eq!(label("custom", |_| None), "CUS");
        let runes = |code: &str| {
            parse_registry(RULES)
                .into_iter()
                .find(|e| e.code == code)
                .map(|e| e.short)
        };
        assert_eq!(label("futhark", runes), "ᚠ");
    }
}
