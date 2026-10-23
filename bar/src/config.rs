//! `~/.config/patina-bar/config.toml`. Every key has a default, so the file
//! can be missing. A broken file never stops the bar: it falls back to the
//! defaults and says why on stderr, and unknown keys are warnings, as with
//! Patina skins.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The modules a side of the bar can hold.
pub const MODULES: &[&str] = &[
    "launcher", "windows", "tray", "tags", "layout", "title", "appid", "clock", "power",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Top,
    Bottom,
}

/// One bar, or its parts apart (experimental): the launcher, the window
/// list and the tray each on a surface of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Bar,
    Split,
}

/// Where a part of the split bar goes: an edge, then where along it. Along
/// the top or bottom it's a row ("bottom-left"); up the left or right side,
/// a column ("left-bottom", the left edge at the bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Spot {
    TopLeft,
    #[serde(alias = "top")]
    TopCenter,
    TopRight,
    BottomLeft,
    #[serde(alias = "bottom")]
    BottomCenter,
    BottomRight,
    LeftTop,
    #[serde(alias = "left", alias = "center-left")]
    LeftCenter,
    LeftBottom,
    RightTop,
    #[serde(alias = "right", alias = "center-right")]
    RightCenter,
    RightBottom,
}

/// Where along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Along {
    Start,
    Middle,
    End,
}

/// A screen edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Top => "top",
            Side::Bottom => "bottom",
            Side::Left => "left",
            Side::Right => "right",
        }
    }
}

impl Spot {
    /// The edge it's along.
    pub fn side(self) -> Side {
        use Spot::*;
        match self {
            TopLeft | TopCenter | TopRight => Side::Top,
            BottomLeft | BottomCenter | BottomRight => Side::Bottom,
            LeftTop | LeftCenter | LeftBottom => Side::Left,
            RightTop | RightCenter | RightBottom => Side::Right,
        }
    }

    /// Where along its edge: the start is the left or the top.
    pub fn along(self) -> Along {
        use Spot::*;
        match self {
            TopLeft | BottomLeft | LeftTop | RightTop => Along::Start,
            TopCenter | BottomCenter | LeftCenter | RightCenter => Along::Middle,
            TopRight | BottomRight | LeftBottom | RightBottom => Along::End,
        }
    }

    /// A column, up the left or right side.
    pub fn vertical(self) -> bool {
        matches!(self.side(), Side::Left | Side::Right)
    }

    /// Across the screen: left, middle or right.
    pub fn x(self) -> Along {
        match self.side() {
            Side::Left => Along::Start,
            Side::Right => Along::End,
            _ => self.along(),
        }
    }

    /// Down the screen: top, middle or bottom.
    pub fn y(self) -> Along {
        match self.side() {
            Side::Top => Along::Start,
            Side::Bottom => Along::End,
            _ => self.along(),
        }
    }

    /// The row in the corner where this column's end meets the top or
    /// bottom (`bottom-left` for `left-bottom`), which it stacks past; none
    /// for a row, or a column in the middle.
    pub fn corner_row(self) -> Option<Spot> {
        use Spot::*;
        Some(match self {
            LeftTop => TopLeft,
            LeftBottom => BottomLeft,
            RightTop => TopRight,
            RightBottom => BottomRight,
            _ => return None,
        })
    }
}

/// What the window tabs show (ADR-0050).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowTabs {
    /// The app's icon and the window's title.
    Both,
    Icons,
    Titles,
}

impl From<WindowTabs> for patina::tabs::TabLabel {
    fn from(tabs: WindowTabs) -> Self {
        match tabs {
            WindowTabs::Both => Self::IconAndTitle,
            WindowTabs::Icons => Self::Icon,
            WindowTabs::Titles => Self::Title,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub position: Position,
    /// Height in logical pixels.
    pub height: u32,
    /// A Patina skin by name: a built-in, or one in ~/.config/patina-bar/skins.
    pub skin: String,
    /// One label per dwl tag, in order. dwl has 9 by default.
    pub tags: Vec<String>,
    /// Show only tags that have windows or are being viewed.
    pub hide_vacant_tags: bool,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    /// What each window's tab shows: "both", "icons" or "titles".
    pub window_tabs: WindowTabs,
    /// "bar", or "split": the launcher, window list and tray apart.
    pub layout: Layout,
    /// Split, where each part goes: an edge, then where along it, as
    /// "bottom-left" (a row) or "left-bottom" (a column); see `Spot`.
    pub launcher_at: Spot,
    pub windows_at: Spot,
    pub tray_at: Spot,
    /// A strftime format, as in date(1).
    pub clock: String,
    /// What the launcher button runs, through sh(1).
    pub launcher: String,
    /// What the tray's speaker runs, through sh(1).
    pub volume: String,
    /// What the tray's Wi-Fi icon runs, through sh(1).
    pub wifi: String,
    /// What the clock runs, through sh(1): its menu, a month and the day's
    /// events from Calendar.
    pub calendar: String,
    /// What the tray's battery runs, through sh(1): Settings' Power and Battery page.
    pub battery: String,
    /// What the power button runs, through sh(1).
    pub power: String,
    /// The desktop's wallpaper, an image file, drawn by patina-wallpaper
    /// (not the bar); "" for the skin's colour.
    pub wallpaper: String,
    /// Apps pinned to the window tabs, by app id, first and in this order
    /// (ADR-0080); read by `patina_system::pins`, as the compositor reads
    /// it too.
    pub pinned: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: Position::Bottom,
            // The height of a Patina window's header bar: its 25 px row,
            // 8 px margins and the 1 px line under it.
            height: 41,
            skin: patina::skin::DEFAULT_SKIN.to_string(),
            tags: (1..=9).map(|n| n.to_string()).collect(),
            hide_vacant_tags: false,
            left: vec![
                "launcher".into(),
                "windows".into(),
                "tags".into(),
                "layout".into(),
            ],
            center: vec!["title".into()],
            right: vec!["tray".into(), "clock".into(), "power".into()],
            window_tabs: WindowTabs::Both,
            layout: Layout::Bar,
            launcher_at: Spot::BottomLeft,
            windows_at: Spot::BottomCenter,
            tray_at: Spot::BottomRight,
            clock: "%a %-d %b  %H:%M".into(),
            launcher: "patina-launcher".into(),
            volume: "patina-volume".into(),
            wifi: "patina-wifi".into(),
            calendar: "patina-clock".into(),
            battery: "patina-settings --page power".into(),
            power: "patina-power".into(),
            wallpaper: String::new(),
            pinned: Vec::new(),
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("patina-bar").join("config.toml"))
}

/// Reads the config at `path`, returning it and anything worth telling the
/// user. A missing file is the defaults, silently.
pub fn load(path: &Path) -> (Config, Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), Vec::new()),
        Err(e) => (
            Config::default(),
            vec![format!("{}: {e}; using the defaults", path.display())],
        ),
    }
}

pub fn parse(text: &str) -> (Config, Vec<String>) {
    let mut warnings = Vec::new();
    let table: toml::Table = match toml::from_str(text) {
        Ok(t) => t,
        Err(e) => return (Config::default(), vec![format!("{e}; using the defaults")]),
    };
    let config: Config = match serde_ignored::deserialize(toml::Value::Table(table), |path| {
        warnings.push(format!("unknown key `{path}`, ignored"))
    }) {
        Ok(c) => c,
        Err(e) => return (Config::default(), vec![format!("{e}; using the defaults")]),
    };
    for module in config
        .left
        .iter()
        .chain(&config.center)
        .chain(&config.right)
    {
        if !MODULES.contains(&module.as_str()) {
            warnings.push(format!(
                "unknown module `{module}`, left out; the modules are {}",
                MODULES.join(", ")
            ));
        }
    }
    (config, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_the_defaults() {
        let (c, w) = parse("");
        assert!(w.is_empty());
        assert_eq!(c.height, 41);
        assert_eq!(c.tags.len(), 9);
        assert_eq!(c.position, Position::Bottom);
        assert_eq!(c.window_tabs, WindowTabs::Both);
    }

    #[test]
    fn window_tabs_show_icons_titles_or_both() {
        assert_eq!(
            parse("window_tabs = \"icons\"").0.window_tabs,
            WindowTabs::Icons
        );
        assert_eq!(
            parse("window_tabs = \"titles\"").0.window_tabs,
            WindowTabs::Titles
        );
        let (c, w) = parse("window_tabs = \"pictures\"");
        assert_eq!(c.window_tabs, WindowTabs::Both);
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn split_reads_its_edges() {
        let (c, w) = parse("layout = \"split\"\nwindows_at = \"left\"\ntray_at = \"top-right\"\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.layout, Layout::Split);
        assert_eq!(c.windows_at, Spot::LeftCenter, "the old name for it");
        assert!(c.windows_at.vertical());
        assert_eq!(c.tray_at, Spot::TopRight);
        assert_eq!(c.launcher_at, Spot::BottomLeft);
        let (c, w) = parse("launcher_at = \"center-right\"\nwindows_at = \"left-bottom\"\n");
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.launcher_at, Spot::RightCenter);
        assert_eq!(
            (c.windows_at.x(), c.windows_at.y()),
            (Along::Start, Along::End)
        );
        assert!(c.windows_at.vertical());
        assert_eq!(c.windows_at.corner_row(), Some(Spot::BottomLeft));
        assert!(!Spot::BottomLeft.vertical());
        assert_eq!(parse("").0.layout, Layout::Bar);
    }

    #[test]
    fn reads_keys() {
        let (c, w) = parse(
            "position = \"top\"\nheight = 30\nright = [\"clock\"]\ntags = [\"web\", \"code\"]\n",
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(c.position, Position::Top);
        assert_eq!(c.height, 30);
        assert_eq!(c.right, ["clock"]);
        assert_eq!(c.tags, ["web", "code"]);
        assert_eq!(c.left, Config::default().left);
    }

    #[test]
    fn unknown_keys_and_modules_are_warnings() {
        let (c, w) = parse("colour = \"red\"\nleft = [\"tags\", \"weather\"]\n");
        assert_eq!(c.left, ["tags", "weather"]);
        assert_eq!(w.len(), 2, "{w:?}");
        assert!(w[0].contains("colour"));
        assert!(w[1].contains("weather"));
    }

    #[test]
    fn a_bad_file_falls_back_to_the_defaults() {
        let (c, w) = parse("height = \"tall\"");
        assert_eq!(c.height, 41);
        assert_eq!(w.len(), 1);
        let (_, w) = parse("height = ");
        assert_eq!(w.len(), 1);
    }
}
