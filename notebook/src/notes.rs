//! The workspace: a folder of markdown notes and subfolders, and the file
//! operations on it.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Folder { path: PathBuf, name: String, children: Vec<Entry> },
    Note { path: PathBuf, name: String },
}

pub fn is_note(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some("md" | "markdown"))
}

/// A note's name as shown: its file name without the extension.
pub fn display_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext @ ("md" | "markdown")) => name.strip_suffix(&format!(".{ext}")).unwrap_or(&name).to_string(),
        _ => name.into_owned(),
    }
}

/// The notes and folders under `dir`: folders first, then notes, each
/// sorted case-insensitively, hidden files skipped.
pub fn scan(dir: &Path) -> Vec<Entry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders = Vec::new();
    let mut notes = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let children = scan(&path);
            folders.push(Entry::Folder { path, name, children });
        } else if is_note(&path) {
            notes.push(Entry::Note { name: display_name(&path), path });
        }
    }
    let key = |e: &Entry| match e {
        Entry::Folder { name, .. } | Entry::Note { name, .. } => name.to_lowercase(),
    };
    folders.sort_by_key(key);
    notes.sort_by_key(key);
    folders.extend(notes);
    folders
}

/// The first note in the tree, depth first, for opening a folder.
pub fn first_note(entries: &[Entry]) -> Option<PathBuf> {
    entries
        .iter()
        .find_map(|e| match e {
            Entry::Note { path, .. } => Some(path.clone()),
            Entry::Folder { .. } => None,
        })
        .or_else(|| {
            entries.iter().find_map(|e| match e {
                Entry::Folder { children, .. } => first_note(children),
                Entry::Note { .. } => None,
            })
        })
}

/// A note file name from what the user typed.
pub fn note_file_name(name: &str) -> String {
    let name = name.trim();
    if is_note(Path::new(name)) { name.to_string() } else { format!("{name}.md") }
}

/// Rejects names that would escape the folder or be hidden.
pub fn valid_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        Err("Enter a name".into())
    } else if name.contains('/') || name.contains('\0') {
        Err("Names can't contain /".into())
    } else if name.starts_with('.') {
        Err("Names can't start with a dot".into())
    } else {
        Ok(())
    }
}

/// Image files the editor can show and insert.
pub fn is_image(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    matches!(ext.as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"))
}

/// How a note in `dir` links to `file`: relative, with `/` and `..` as
/// needed, or absolute when they share no root.
pub fn relative_src(dir: &Path, file: &Path) -> String {
    let common = dir.components().zip(file.components()).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return file.to_string_lossy().into_owned();
    }
    let ups = dir.components().count() - common;
    let rest = file.components().skip(common).map(|c| c.as_os_str().to_string_lossy().into_owned());
    std::iter::repeat_n("..".to_string(), ups).chain(rest).collect::<Vec<_>>().join("/")
}

/// `dir/name`, or `dir/name 2`, `dir/name 3`... if it's taken.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} {n}{ext}"))).find(|p| !p.exists()).expect("some number is free")
}

/// Writes through a temporary file and a rename, so a crash mid-save never
/// leaves a half-written note.
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.notebook-tmp", path.file_name().unwrap_or_default().to_string_lossy()));
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// Where `path` ends up when `from` is renamed or moved to `to` (`path`
/// may be `from` itself or inside it).
pub fn moved_path(path: &Path, from: &Path, to: &Path) -> PathBuf {
    match path.strip_prefix(from) {
        Ok(rest) if rest.as_os_str().is_empty() => to.to_path_buf(),
        Ok(rest) => to.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_folders_first_then_notes() {
        let dir = std::env::temp_dir().join(format!("notebook-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Work/Deep")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        for f in ["b.md", "A.md", "notes.txt", "Work/plan.markdown"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let entries = scan(&dir);
        let names: Vec<String> = entries
            .iter()
            .map(|e| match e {
                Entry::Folder { name, .. } | Entry::Note { name, .. } => name.clone(),
            })
            .collect();
        assert_eq!(names, ["Work", "A", "b"]);
        assert_eq!(first_note(&entries), Some(dir.join("A.md")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names() {
        assert_eq!(note_file_name("Groceries "), "Groceries.md");
        assert_eq!(note_file_name("x.md"), "x.md");
        assert_eq!(display_name(Path::new("/n/Plan.md")), "Plan");
        assert!(valid_name("a/b").is_err());
        assert!(valid_name(".x").is_err());
        assert!(valid_name("  ").is_err());
    }

    #[test]
    fn image_paths() {
        let dir = Path::new("/n/Work");
        assert_eq!(relative_src(dir, Path::new("/n/Work/a b.png")), "a b.png");
        assert_eq!(relative_src(dir, Path::new("/n/Pics/x/a.png")), "../Pics/x/a.png");
        assert!(is_image(Path::new("a.JPG")) && !is_image(Path::new("a.md")));
    }

    #[test]
    fn free_paths_number_taken_names() {
        let dir = std::env::temp_dir().join(format!("notebook-free-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.png"), dir.join("a.png"));
        std::fs::write(dir.join("a.png"), "").unwrap();
        assert_eq!(free_path(&dir, "a.png"), dir.join("a 2.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn moved_paths_follow_their_folder() {
        let (from, to) = (Path::new("/n/Work"), Path::new("/n/Archive/Work"));
        assert_eq!(moved_path(Path::new("/n/Work/a.md"), from, to), Path::new("/n/Archive/Work/a.md"));
        assert_eq!(moved_path(from, from, to), to);
        assert_eq!(moved_path(Path::new("/n/Other.md"), from, to), Path::new("/n/Other.md"));
    }
}
