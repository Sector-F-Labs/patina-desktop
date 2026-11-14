//! `~/.config/patina-notify/config.toml` (ADR-0141), defined in
//! `patina_system::notifications`, where the tray's bell and Settings' page
//! write it too. The column checks its time each second.

use std::time::SystemTime;

pub use patina_system::notifications::Config;
use patina_system::notifications::{parse, path};

/// Re-reads the file when its time has changed since `seen`.
pub fn reload(seen: &mut Option<SystemTime>) -> Option<Config> {
    let path = path()?;
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    if modified == *seen {
        return None;
    }
    *seen = modified;
    Some(
        std::fs::read_to_string(&path)
            .map(|t| parse(&t))
            .unwrap_or_default(),
    )
}
