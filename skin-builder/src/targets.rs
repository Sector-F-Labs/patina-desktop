//! The apps a skin can be applied to: the folders in `~/.config`. A Patina app keeps its skins
//! in `<folder>/skins` and its choice as `skin = "…"` in `<folder>/config.toml`, so applying is
//! writing the file and setting the choice. The app picks it up when it next starts.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, value};

pub struct Target {
    /// The folder's name.
    pub name: String,
    pub dir: PathBuf,
    /// It looks like a Patina app's: a `skins` folder, or a `skin` in its config.
    pub patina: bool,
    /// The skin its config names now.
    pub skin: Option<String>,
}

fn config_file(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

fn chosen(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(config_file(dir)).ok()?;
    let doc = text.parse::<DocumentMut>().ok()?;
    doc.get("skin")?.as_str().map(str::to_string)
}

/// The folders in `config`, Patina apps first, then by name; hidden ones and loose files left out.
pub fn find(config: &Path) -> Vec<Target> {
    let Ok(entries) = std::fs::read_dir(config) else {
        return Vec::new();
    };
    let mut found: Vec<Target> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let dir = e.path();
            let skin = chosen(&dir);
            let patina = skin.is_some() || dir.join("skins").is_dir();
            Some(Target {
                name,
                dir,
                patina,
                skin,
            })
        })
        .collect();
    found.sort_by_key(|t| (!t.patina, t.name.to_lowercase()));
    found
}

/// Puts the skin `text`, named `skin`, in `target`'s skins folder as `file`, replacing one there,
/// and makes it the app's choice. A config that won't parse is left as it is, and it's an error.
pub fn apply(target: &Target, file: &str, text: &str, skin: &str) -> Result<(), String> {
    let config = config_file(&target.dir);
    let existing = match std::fs::read_to_string(&config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", config.display())),
    };
    let mut doc = existing
        .parse::<DocumentMut>()
        .map_err(|e| format!("{}: {e}", config.display()))?;
    let skins = target.dir.join("skins");
    std::fs::create_dir_all(&skins).map_err(|e| format!("{}: {e}", skins.display()))?;
    let path = skins.join(file);
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    doc["skin"] = value(skin);
    std::fs::write(&config, doc.to_string()).map_err(|e| format!("{}: {e}", config.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("skin-builder-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_folders_and_which_are_patina_apps() {
        let config = scratch("find");
        std::fs::create_dir_all(config.join("notebook/skins")).unwrap();
        std::fs::write(config.join("notebook/config.toml"), "skin = \"Notebook\"\n").unwrap();
        std::fs::create_dir_all(config.join("git")).unwrap();
        std::fs::create_dir_all(config.join(".hidden")).unwrap();
        std::fs::write(config.join("loose.conf"), "").unwrap();
        let found = find(&config);
        let names: Vec<_> = found.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["notebook", "git"]);
        assert!(found[0].patina && !found[1].patina);
        assert_eq!(found[0].skin.as_deref(), Some("Notebook"));
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn applying_writes_the_skin_and_keeps_the_rest_of_the_config() {
        let config = scratch("apply");
        let dir = config.join("files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "view = \"Icons\"\nskin = \"Old\"\n\n[[tabs]]\nroot = \"/\"\n",
        )
        .unwrap();
        let target = find(&config).remove(0);
        apply(
            &target,
            "mine.toml",
            "format = 1\nname = \"Mine\"\n",
            "Mine",
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert!(text.contains("skin = \"Mine\"") && text.contains("view = \"Icons\""));
        assert!(text.contains("[[tabs]]"));
        assert!(dir.join("skins/mine.toml").is_file());
        // A folder with no config gets one.
        let bare = config.join("newapp");
        std::fs::create_dir_all(&bare).unwrap();
        let target = Target {
            name: "newapp".into(),
            dir: bare.clone(),
            patina: false,
            skin: None,
        };
        apply(&target, "mine.toml", "x", "Mine").unwrap();
        assert_eq!(chosen(&bare).as_deref(), Some("Mine"));
        // A config that doesn't parse is left alone.
        std::fs::write(bare.join("config.toml"), "this is = = not toml").unwrap();
        assert!(apply(&target, "mine.toml", "x", "Mine").is_err());
        assert_eq!(
            std::fs::read_to_string(bare.join("config.toml")).unwrap(),
            "this is = = not toml"
        );
        let _ = std::fs::remove_dir_all(config);
    }
}
