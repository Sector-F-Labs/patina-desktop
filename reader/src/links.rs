//! Where a link in a document goes: a web page, another file Reader can
//! show (opened in Reader), a folder (its README), or some other file
//! (opened in the desktop's app for it).

use std::path::{Component, Path, PathBuf};

use crate::tree::Kind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A URL with a scheme: the web browser's.
    Web(String),
    /// A file Reader shows; the `#fragment`, if any, isn't followed yet.
    Read(PathBuf),
    /// A file Reader doesn't show, or a folder with nothing to read.
    Open(PathBuf),
    /// A link to a place in this document (`#heading`), or nowhere.
    Stay,
}

/// What clicking `link` in the document `from` does.
pub fn resolve(link: &str, from: &Path) -> Target {
    let link = link.trim();
    if link.contains("://") || link.starts_with("mailto:") {
        return Target::Web(link.to_string());
    }
    let path = link.split('#').next().unwrap_or_default();
    if path.is_empty() {
        return Target::Stay;
    }
    let dir = from.parent().unwrap_or(Path::new("/"));
    let target = normalise(&dir.join(percent_decode(path)));
    if target.is_dir() {
        return match crate::tree::first_file(&crate::tree::scan(&target)) {
            Some(readme) if readme.parent() == Some(&target) => Target::Read(readme),
            _ => Target::Open(target),
        };
    }
    match Kind::of(&target) {
        Some(_) => Target::Read(target),
        None => Target::Open(target),
    }
}

/// `a/./b/../c` as `a/c`, without touching the disk.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `%20`-style escapes, which markdown links often use for spaces.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_go_where_they_point() {
        let root = std::env::temp_dir().join(format!("reader-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/adr")).unwrap();
        std::fs::create_dir_all(root.join("bar/src")).unwrap();
        std::fs::write(root.join("bar/README.md"), "").unwrap();
        std::fs::write(root.join("bar/src/main.rs"), "").unwrap();
        std::fs::write(root.join("docs/adr/0001-a b.md"), "").unwrap();
        std::fs::write(root.join("docs/shot.png"), "").unwrap();
        let from = root.join("README.md");

        assert_eq!(resolve("https://github.com/x", &from), Target::Web("https://github.com/x".into()));
        assert_eq!(resolve("#usage", &from), Target::Stay);
        assert_eq!(resolve("docs/adr/0001-a%20b.md#context", &from), Target::Read(root.join("docs/adr/0001-a b.md")));
        assert_eq!(resolve("bar/src/main.rs", &from), Target::Read(root.join("bar/src/main.rs")));
        assert_eq!(resolve("bar/", &from), Target::Read(root.join("bar/README.md")), "a folder shows its README");
        assert_eq!(resolve("docs/shot.png", &from), Target::Open(root.join("docs/shot.png")));
        let deep = root.join("docs/adr/0001-a b.md");
        assert_eq!(resolve("../../bar/README.md", &deep), Target::Read(root.join("bar/README.md")));
        std::fs::remove_dir_all(&root).ok();
    }
}
