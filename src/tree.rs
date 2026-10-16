//! The project as the sidebar shows it: folders and the files Reader can
//! show (markdown, man pages and Rust), read when a project opens and again
//! when its folders change. Folders with nothing readable in them are left
//! out, as are build output, version control and hidden folders.

use std::path::{Path, PathBuf};

/// The kinds of file Reader shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    /// A man page's source, formatted by the system's man tools.
    Man,
    Rust,
}

impl Kind {
    pub fn of(path: &Path) -> Option<Kind> {
        if crate::man::is_page(path) {
            return Some(Kind::Man);
        }
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "md" | "markdown" => Some(Kind::Markdown),
            "rs" => Some(Kind::Rust),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Folder { name: String, path: PathBuf, children: Vec<Node> },
    File { name: String, path: PathBuf, kind: Kind },
}

impl Node {
    pub fn name(&self) -> &str {
        match self {
            Node::Folder { name, .. } | Node::File { name, .. } => name,
        }
    }
}

/// Folders never shown: build output and dependencies, which hold copies
/// and generated files rather than the project's own.
const SKIPPED: &[&str] = &["target", "node_modules", "vendor", "build", "dist", "out"];

/// The readable files under `root`, folders first, each level sorted with
/// READMEs at the top.
pub fn scan(root: &Path) -> Vec<Node> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut nodes: Vec<Node> = entries
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                if skipped(&name) {
                    return None;
                }
                let children = scan(&path);
                (!children.is_empty()).then_some(Node::Folder { name, path, children })
            } else {
                let kind = Kind::of(&path)?;
                Some(Node::File { name, path, kind })
            }
        })
        .collect();
    nodes.sort_by(|a, b| order(a).cmp(&order(b)).then_with(|| a.name().to_lowercase().cmp(&b.name().to_lowercase())));
    nodes
}

/// `root` and every folder under it that `scan` looks in, empty or not,
/// as a file added to any of them shows in the tree.
pub fn folders(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.to_path_buf()];
    let mut i = 0;
    while let Some(dir) = found.get(i).cloned() {
        i += 1;
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        found.extend(
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| !skipped(&e.file_name().to_string_lossy()))
                .map(|e| e.path()),
        );
    }
    found
}

/// Hidden folders, and build output and dependencies.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || SKIPPED.contains(&name)
}

/// Every man page on the system, a folder per section in order ("1 User
/// Commands"), each page named without its section. Where the same page
/// is in more than one place, the first on the manpath wins, as for man.
pub fn man_pages() -> Vec<Node> {
    let mut sections: std::collections::BTreeMap<char, std::collections::BTreeMap<String, PathBuf>> =
        Default::default();
    for dir in crate::man::manpath() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut subdirs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        subdirs.sort();
        for sub in subdirs {
            // man1 to man9 (and man3p and the like), not the translations.
            let Some(section) = sub
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("man"))
                .and_then(|n| n.chars().next())
                .filter(|c| ('1'..='9').contains(c))
            else {
                continue;
            };
            let Ok(pages) = std::fs::read_dir(&sub) else { continue };
            let section = sections.entry(section).or_default();
            for page in pages.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| crate::man::is_page(p)) {
                section.entry(crate::man::page_name(&page)).or_insert(page);
            }
        }
    }
    sections
        .into_iter()
        .filter(|(_, pages)| !pages.is_empty())
        .map(|(section, pages)| {
            let mut children: Vec<Node> =
                pages.into_iter().map(|(name, path)| Node::File { name, path, kind: Kind::Man }).collect();
            children.sort_by_key(|n| n.name().to_lowercase());
            Node::Folder {
                name: format!("{section} {}", crate::man::section_title(section)),
                path: PathBuf::from(format!("man:{section}")),
                children,
            }
        })
        .collect()
}

/// Folders, then READMEs, then the other markdown, man pages, then code.
fn order(node: &Node) -> u8 {
    match node {
        Node::Folder { .. } => 0,
        Node::File { name, .. } if name.to_lowercase().starts_with("readme") => 1,
        Node::File { kind: Kind::Markdown, .. } => 2,
        Node::File { kind: Kind::Man, .. } => 3,
        Node::File { kind: Kind::Rust, .. } => 4,
    }
}

/// The file to show first in a project or folder: its README, else its
/// first markdown file, else anything.
pub fn first_file(nodes: &[Node]) -> Option<PathBuf> {
    let files = || {
        nodes.iter().filter_map(|n| match n {
            Node::File { path, kind, .. } => Some((path, *kind)),
            Node::Folder { .. } => None,
        })
    };
    files()
        .find(|(p, _)| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().starts_with("readme")))
        .or_else(|| files().find(|(_, k)| *k == Kind::Markdown))
        .or_else(|| files().next())
        .map(|(p, _)| p.clone())
        .or_else(|| {
            nodes.iter().find_map(|n| match n {
                Node::Folder { children, .. } => first_file(children),
                Node::File { .. } => None,
            })
        })
}

/// The nodes whose name contains `filter` (case aside), with the folders
/// that lead to them; everything when it's empty.
pub fn filtered(nodes: &[Node], filter: &str) -> Vec<Node> {
    let filter = filter.trim().to_lowercase();
    if filter.is_empty() {
        return nodes.to_vec();
    }
    nodes
        .iter()
        .filter_map(|node| match node {
            Node::File { name, .. } => name.to_lowercase().contains(&filter).then(|| node.clone()),
            Node::Folder { name, path, children } => {
                let kept = filtered(children, &filter);
                (!kept.is_empty()).then(|| Node::Folder { name: name.clone(), path: path.clone(), children: kept })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("reader-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in [
            ("README.md", "# Hi"),
            ("Cargo.toml", "[package]"),
            ("src/main.rs", "fn main() {}"),
            ("src/lib.rs", ""),
            ("docs/adr/0001-x.md", "# X"),
            ("docs/screenshot.png", ""),
            ("target/debug/out.rs", ""),
            (".git/HEAD.md", ""),
            ("empty/notes.txt", ""),
            ("ARCHITECTURE.md", ""),
            ("man/tool.1", ""),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .map(|n| match n {
                Node::Folder { name, children, .. } => format!("{name}/[{}]", names(children).join(",")),
                Node::File { name, .. } => name.clone(),
            })
            .collect()
    }

    #[test]
    fn shows_only_readable_files_folders_first_readmes_on_top() {
        let root = project("scan");
        let nodes = scan(&root);
        assert_eq!(
            names(&nodes),
            ["docs/[adr/[0001-x.md]]", "man/[tool.1]", "src/[lib.rs,main.rs]", "README.md", "ARCHITECTURE.md"]
        );
        assert_eq!(first_file(&nodes), Some(root.join("README.md")));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn filtering_keeps_the_way_to_matches() {
        let root = project("filter");
        let nodes = scan(&root);
        assert_eq!(names(&filtered(&nodes, "MAIN")), ["src/[main.rs]"]);
        assert_eq!(names(&filtered(&nodes, "0001")), ["docs/[adr/[0001-x.md]]"]);
        assert!(filtered(&nodes, "nothing").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_systems_man_pages_by_section() {
        let sections = man_pages();
        // Every system Reader runs on has section 1.
        let Some(Node::Folder { name, children, .. }) = sections.first() else { panic!("no sections") };
        assert_eq!(name, "1 User Commands");
        assert!(children.iter().any(|c| c.name() == "ls"), "ls is in section 1");
        assert!(children.iter().all(|c| matches!(c, Node::File { kind: Kind::Man, .. })));
    }

    #[test]
    fn folders_are_those_scanned_empty_or_not() {
        let root = project("folders");
        let mut found: Vec<String> = folders(&root)
            .iter()
            .map(|f| f.strip_prefix(&root).unwrap().display().to_string())
            .collect();
        found.sort();
        assert_eq!(found, ["", "docs", "docs/adr", "empty", "man", "src"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_folder_without_a_readme_starts_at_its_first_file() {
        let root = project("first");
        let src = scan(&root.join("src"));
        assert_eq!(first_file(&src), Some(root.join("src/lib.rs")));
        std::fs::remove_dir_all(&root).ok();
    }
}
