//! Where each app's window was last, by app id (ADR-0065), kept in
//! `~/.local/state/patina-tiler/windows.toml` as `"app.id" = { x, y }`, with
//! `filled = true` if it closed filling the work area.
//! Deleting the file forgets them.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::policy::Remembered;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Spot {
    x: i32,
    y: i32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    filled: bool,
}

#[derive(Default)]
pub struct Positions {
    path: Option<PathBuf>,
    by_app: BTreeMap<String, Spot>,
}

fn path() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(|| Some(dirs::home_dir()?.join(".local").join("state")))
        .map(|d| d.join("patina-tiler").join("windows.toml"))
}

impl Positions {
    pub fn load() -> Self {
        let path = path();
        let by_app = path
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| match toml::from_str(&text) {
                Ok(by_app) => Some(by_app),
                Err(e) => {
                    eprintln!("patina-tiler: ignoring windows.toml: {e}");
                    None
                }
            })
            .unwrap_or_default();
        Positions { path, by_app }
    }

    pub fn get(&self, app_id: &str) -> Option<Remembered> {
        self.by_app.get(app_id).map(|s| Remembered {
            at: (s.x, s.y),
            filled: s.filled,
        })
    }

    /// Remembers `app_id`'s window, writing the file if that changed
    /// anything.
    pub fn set(&mut self, app_id: &str, remembered: Remembered) {
        let Remembered { at: (x, y), filled } = remembered;
        let spot = Spot { x, y, filled };
        if app_id.is_empty() || self.by_app.get(app_id) == Some(&spot) {
            return;
        }
        self.by_app.insert(app_id.to_string(), spot);
        if let Some(path) = &self.path
            && let Err(e) = save(path, &self.by_app)
        {
            eprintln!("patina-tiler: can't save {}: {e}", path.display());
        }
    }
}

fn save(path: &Path, by_app: &BTreeMap<String, Spot>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = toml::to_string(by_app).map_err(std::io::Error::other)?;
    // Written beside it and renamed over it, so a crash never leaves half.
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("patina-tiler-{}", std::process::id()));
        let mut positions = Positions {
            path: Some(dir.join("windows.toml")),
            by_app: BTreeMap::new(),
        };
        let floated = Remembered {
            at: (10, -20),
            filled: false,
        };
        positions.set("org.gnome.Nautilus", floated);
        positions.set(
            "foot",
            Remembered {
                at: (5, 5),
                filled: true,
            },
        );
        positions.set("", floated);
        let text = std::fs::read_to_string(dir.join("windows.toml")).unwrap();
        assert!(!text.contains("filled = false"), "only when it's true");
        let read: BTreeMap<String, Spot> = toml::from_str(&text).unwrap();
        assert_eq!(read.len(), 2, "no empty app id");
        assert_eq!(
            read["org.gnome.Nautilus"],
            Spot {
                x: 10,
                y: -20,
                filled: false
            }
        );
        assert!(read["foot"].filled);
        let _ = std::fs::remove_dir_all(dir);
    }
}
