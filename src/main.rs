//! Reader: a Patina reader for a project's documentation and code. Point it
//! at a folder and browse its markdown, shown as Notebook shows it, its man
//! pages, and its Rust, highlighted; links between them are followed. It's meant to become
//! Patina's help viewer.

mod app;
mod config;
mod links;
mod man;
mod tree;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: reader [FOLDER | FILE]

Opens FOLDER as a project to read, or the project FILE is in (the nearest
folder above it with a .git, else its own folder) at FILE. Without either,
the last project read. Markdown (.md), man pages (.1 to .9) and Rust (.rs)
files are shown.";

fn main() -> ExitCode {
    let mut asked = None;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("reader {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            _ if asked.is_none() => {
                let path = arg.strip_prefix("file://").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&arg));
                asked = Some(std::path::absolute(path).unwrap_or_default());
            }
            _ => {}
        }
    }
    let (root, file) = match asked {
        Some(path) if path.is_dir() => (Some(path), None),
        Some(path) if path.is_file() => (Some(project_of(&path)), Some(path)),
        Some(path) => {
            eprintln!("reader: there's nothing at {}", path.display());
            return ExitCode::from(2);
        }
        None => (None, None),
    };
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Reader")
                .with_app_id("com.sectorflabs.Reader")
                .with_inner_size([1100.0, 760.0])
                .with_min_inner_size([560.0, 360.0]),
            true,
        ),
        ..Default::default()
    };
    match eframe::run_native("reader", options, Box::new(|cc| Ok(Box::new(app::Reader::new(cc, root, file))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("reader: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The project a file belongs to: the nearest folder above it with a
/// `.git`, else the file's own folder.
fn project_of(file: &Path) -> PathBuf {
    let own = file.parent().unwrap_or(Path::new("/")).to_path_buf();
    file.ancestors().skip(1).find(|dir| dir.join(".git").exists()).map_or(own, Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_opens_its_repository() {
        let root = std::env::temp_dir().join(format!("reader-main-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/a.md"), "").unwrap();
        assert_eq!(project_of(&root.join("docs/a.md")), root);
        std::fs::remove_dir_all(&root).ok();
        let lone = std::env::temp_dir().join(format!("reader-lone-{}", std::process::id()));
        std::fs::create_dir_all(&lone).unwrap();
        std::fs::write(lone.join("b.md"), "").unwrap();
        assert_eq!(project_of(&lone.join("b.md")), lone);
        std::fs::remove_dir_all(&lone).ok();
    }
}
