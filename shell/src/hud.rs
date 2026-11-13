//! The bar's skin and edge, for shell pieces the bar opens (the launcher,
//! the volume menu) so they match it and open from it. The bar passes them
//! as PATINA_SKIN and PATINA_HUD_EDGE; when something else starts a piece
//! (the compositor, for the Super key or a volume key), they come from the
//! bar's config file instead.

/// The shell's skin when the bar's config sets none: Everforest Dark, one of
/// Patina's built-ins. Patina's own default (`Default`) stays the one for
/// Patina apps.
pub const DEFAULT_SKIN: &str = "Everforest Dark";

/// The bar's skin name, if any is set.
pub fn skin() -> Option<String> {
    std::env::var("PATINA_SKIN")
        .ok()
        .or_else(|| config_value("skin"))
}

/// The bar's skin name, or [`DEFAULT_SKIN`] when none is set.
pub fn skin_or_default() -> String {
    skin().unwrap_or_else(|| DEFAULT_SKIN.to_string())
}

/// Whether the bar is at the top of the screen (the default is the bottom).
pub fn at_top() -> bool {
    std::env::var("PATINA_HUD_EDGE")
        .ok()
        .or_else(|| config_value("position"))
        .as_deref()
        == Some("top")
}

/// The corner radius of the HUD's islands, which the menus share: the
/// compositor's `[gaps]` radius while the HUD floats, else 8, as the bar draws it.
pub fn radius() -> f32 {
    static RADIUS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *RADIUS.get_or_init(|| {
        let gaps = crate::gaps::read();
        if gaps.floating() { gaps.radius } else { 8.0 }
    })
}

/// [`radius`] as egui's corner radius, for a menu's card.
pub fn corner_radius() -> egui::CornerRadius {
    egui::CornerRadius::from(radius())
}

/// Gives `visuals` the HUD's corners and shadow for the menus and popups egui
/// draws itself, on a surface that's all screen (patina-menu), where a
/// shadow can't be asked of the compositor around a card.
pub fn style_popups(visuals: &mut egui::Visuals) {
    visuals.menu_corner_radius = corner_radius();
    visuals.popup_shadow = match shadow() {
        Some(s) => egui::Shadow {
            offset: [s.offset_x.round() as i8, s.drop.round() as i8],
            blur: s.blur.round() as u8,
            spread: s.spread.round().max(0.0) as u8,
            color: egui::Color32::from_rgba_unmultiplied(
                s.color[0],
                s.color[1],
                s.color[2],
                (s.opacity * 255.0).round() as u8,
            ),
        },
        None => egui::Shadow::NONE,
    };
}

/// The shadow the HUD's config asks for around its parts (ADR-0113), for the
/// menus to ask for too; `None` when `shadows = false`. A key that's missing
/// or unreadable is the default, as for the HUD.
pub fn shadow() -> Option<crate::ShadowStyle> {
    let text = std::fs::read_to_string(config_path()?).unwrap_or_default();
    shadow_from(&text)
}

fn shadow_from(text: &str) -> Option<crate::ShadowStyle> {
    if top_level(text, "shadows").as_deref() == Some("false") {
        return None;
    }
    let default = crate::ShadowStyle::default();
    let number = |key: &str, default: f32| {
        top_level(text, key)
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|n| n.is_finite())
            .unwrap_or(default)
    };
    let color = top_level(text, "shadow_color")
        .and_then(|c| {
            let hex = c.strip_prefix('#')?;
            if hex.len() != 6 || !hex.is_ascii() {
                return None;
            }
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some([byte(0)?, byte(2)?, byte(4)?])
        })
        .unwrap_or([0, 0, 0]);
    Some(crate::ShadowStyle {
        blur: number("shadow_blur", default.blur).clamp(0.0, 64.0),
        drop: number("shadow_drop", default.drop).clamp(-64.0, 64.0),
        opacity: number("shadow_opacity", default.opacity).clamp(0.0, 1.0),
        spread: number("shadow_spread", default.spread).clamp(-64.0, 64.0),
        offset_x: number("shadow_offset_x", default.offset_x).clamp(-64.0, 64.0),
        color,
    })
}

/// The HUD's config file, `~/.config/patina-hud/config.toml`. The folder was
/// `patina-bar` before the bar became the HUD (ADR-0110): when only the old
/// one exists it is moved to the new name, skins and all.
pub fn config_path() -> Option<std::path::PathBuf> {
    let base = dirs_config()?;
    let dir = base.join("patina-hud");
    let old = base.join("patina-bar");
    if !dir.exists() && old.is_dir() {
        let _ = std::fs::rename(&old, &dir);
    }
    Some(dir.join("config.toml"))
}

fn config_value(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(config_path()?).ok()?;
    top_level(&text, key)
}

/// The desktop's wallpaper, an image file; `None` for the skin's colour.
pub fn wallpaper() -> Option<std::path::PathBuf> {
    config_value("wallpaper")
        .filter(|p| !p.is_empty())
        .map(std::path::PathBuf::from)
}

/// Sets the top-level `key` (`skin`, `wallpaper`) in the bar's config file,
/// keeping the rest of the file, and making the file if there's none. The
/// bar and the wallpaper notice; pieces pick it up when they next open.
pub fn set(key: &str, value: &str) -> Result<(), String> {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    set_raw(key, &format!("\"{escaped}\""))
}

/// Sets the top-level `key` to a TOML boolean (`shadows`), as [`set`] sets a
/// string.
pub fn set_bool(key: &str, value: bool) -> Result<(), String> {
    set_raw(key, if value { "true" } else { "false" })
}

/// Sets the top-level `key` to a TOML number (`shadow_blur`), as [`set`] sets
/// a string: a float, so a whole one is written `8.0`.
pub fn set_number(key: &str, value: f64) -> Result<(), String> {
    set_raw(key, &format!("{value:?}"))
}

/// [`set`] with `toml` as the value, already as TOML says it.
fn set_raw(key: &str, toml: &str) -> Result<(), String> {
    let path = config_path().ok_or("no home folder")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, with_top_level_raw(&text, key, toml))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// `text` with the top-level `key` set to the string `value`: the line
/// that sets it replaced, or a new one before the first [section].
pub fn with_top_level(text: &str, key: &str, value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    with_top_level_raw(text, key, &format!("\"{escaped}\""))
}

/// [`with_top_level`] with `toml` as the value, already as TOML says it.
fn with_top_level_raw(text: &str, key: &str, toml: &str) -> String {
    let line = format!("{key} = {toml}");
    let mut out = Vec::new();
    let mut done = false;
    for l in text.lines() {
        let trimmed = l.trim();
        if !done && trimmed.starts_with('[') {
            out.push(line.clone());
            done = true;
        }
        let sets_key = trimmed
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == key);
        if !done && sets_key {
            out.push(line.clone());
            done = true;
        } else {
            out.push(l.to_string());
        }
    }
    if !done {
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    text
}

/// `key = "value"` at the top of a TOML file, before any [section].
pub fn top_level(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            return None;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() == key {
            let v = v.trim();
            // A quoted value may hold a `#`; otherwise one starts a comment.
            let v = match v.strip_prefix('"').and_then(|r| r.split_once('"')) {
                Some((quoted, _)) => quoted,
                None => v.split('#').next()?.trim(),
            };
            return Some(v.to_string());
        }
    }
    None
}

fn dirs_config() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_value_may_hold_a_hash() {
        let text = "a = \"#1a2b3c\" # a colour\nb = 2 # two\nc = \"x\"\n";
        assert_eq!(top_level(text, "a").as_deref(), Some("#1a2b3c"));
        assert_eq!(top_level(text, "b").as_deref(), Some("2"));
        assert_eq!(top_level(text, "c").as_deref(), Some("x"));
    }

    #[test]
    fn the_menus_shadow_is_the_huds() {
        assert_eq!(shadow_from(""), Some(crate::ShadowStyle::default()));
        assert_eq!(shadow_from("shadows = false\n"), None);
        let style =
            shadow_from("shadow_blur = 3\nshadow_color = \"#102030\"\nshadow_opacity = 7\n")
                .unwrap();
        assert_eq!(
            (style.blur, style.opacity, style.color),
            (3.0, 1.0, [0x10, 0x20, 0x30])
        );
    }

    #[test]
    fn the_default_skin_is_built_in() {
        let (skins, errors) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        assert!(errors.is_empty());
        assert!(skins.iter().any(|s| s.name == DEFAULT_SKIN));
    }

    #[test]
    fn reads_top_level_keys_only() {
        let text = "# bar\nposition = \"top\"   # or bottom\nskin = \"Steam Classic\"\n[other]\nskin = \"x\"\n";
        assert_eq!(top_level(text, "position").as_deref(), Some("top"));
        assert_eq!(top_level(text, "skin").as_deref(), Some("Steam Classic"));
        assert_eq!(top_level("[a]\nskin = \"x\"", "skin"), None);
        assert_eq!(top_level("", "skin"), None);
    }

    #[test]
    fn sets_a_top_level_key_keeping_the_rest() {
        let text = "# bar\nskin = \"Default\"  # old\nheight = 30\n[other]\nskin = \"x\"\n";
        assert_eq!(
            with_top_level(text, "skin", "Steam Classic"),
            "# bar\nskin = \"Steam Classic\"\nheight = 30\n[other]\nskin = \"x\"\n"
        );
        assert_eq!(
            with_top_level("height = 30\n[a]\nb = 1\n", "skin", "Aqua"),
            "height = 30\nskin = \"Aqua\"\n[a]\nb = 1\n"
        );
        assert_eq!(
            with_top_level("", "skin", "A \"q\""),
            "skin = \"A \\\"q\\\"\"\n"
        );
        let set = with_top_level("# none yet\n", "skin", "Aqua");
        assert_eq!(top_level(&set, "skin").as_deref(), Some("Aqua"));
    }
}
