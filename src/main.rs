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

const USAGE: &str = "usage: reader [-s | --single] [FOLDER | FILE] [--file FILE]
       reader --man [PAGE]

Opens FOLDER as a project to read, or the project FILE is in (the nearest
folder above it with a .git, else its own folder) at FILE. --file FILE names
the file to open, inside FOLDER when one is given too. Without either, the
last project read. Markdown (.md), man pages (.1 to .9) and Rust (.rs) files
are shown.

-s, --single   single-file mode: the file alone, without the project tree,
               in a smaller window; links and Back still work, and the last
               project read isn't changed. A FOLDER opens its README.
--man          system mode: the tree is every man page on the system, by
               section, and only that; PAGE (ls, printf(3)) opens first,
               else man(1).";

/// What the command line asks for.
#[derive(Debug, Default, PartialEq)]
struct Args {
    path: Option<PathBuf>,
    file: Option<PathBuf>,
    single: bool,
    /// System mode, perhaps with a page to open.
    man: bool,
    page: Option<String>,
}

enum Parsed {
    Run(Args),
    Help,
    Version,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut out = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "-s" | "--single" => out.single = true,
            "--man" => out.man = true,
            "--file" => {
                let file = args.next().ok_or("--file needs a file")?;
                out.file = Some(as_path(&file));
            }
            a if a.starts_with("--file=") => out.file = Some(as_path(&a["--file=".len()..])),
            a if a.starts_with('-') && a != "-" => return Err(format!("unknown option {a}")),
            _ if out.man && out.page.is_none() => out.page = Some(arg),
            _ if !out.man && out.path.is_none() => out.path = Some(as_path(&arg)),
            _ => return Err(format!("one path at a time: {arg}")),
        }
    }
    if out.man {
        // A path given before --man was a page name.
        if let Some(path) = out.path.take() {
            out.page = out.page.or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
    }
    Ok(Parsed::Run(out))
}

/// An argument as an absolute path; `file://` URIs as their path.
fn as_path(arg: &str) -> PathBuf {
    let path = arg.strip_prefix("file://").map_or_else(|| PathBuf::from(arg), PathBuf::from);
    std::path::absolute(&path).unwrap_or(path)
}

/// The project and file to open.
fn resolve(args: &Args) -> Result<(Option<PathBuf>, Option<PathBuf>), String> {
    let missing = |p: &Path| format!("there's nothing at {}", p.display());
    if let Some(file) = &args.file
        && !file.is_file()
    {
        return Err(missing(file));
    }
    match (&args.path, &args.file) {
        (Some(path), file) if path.is_dir() => Ok((Some(path.clone()), file.clone())),
        (Some(path), None) if path.is_file() => Ok((Some(project_of(path)), Some(path.clone()))),
        (Some(path), Some(_)) if path.is_file() => Err("a FILE and --file: give one".into()),
        (Some(path), _) => Err(missing(path)),
        (None, Some(file)) => Ok((Some(project_of(file)), Some(file.clone()))),
        (None, None) => Ok((None, None)),
    }
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1)) {
        Ok(Parsed::Run(args)) => args,
        Ok(Parsed::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Parsed::Version) => {
            println!("reader {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("reader: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.man {
        let page = match &args.page {
            Some(name) => match man::find(name) {
                Some(page) => Some(page),
                None => {
                    eprintln!("reader: no man page for {name}");
                    return ExitCode::from(2);
                }
            },
            None => None,
        };
        return run(None, page, args.single, true);
    }
    let (root, file) = match resolve(&args) {
        Ok(found) => found,
        Err(e) => {
            eprintln!("reader: {e}");
            return ExitCode::from(2);
        }
    };
    run(root, file, args.single, false)
}

fn run(root: Option<PathBuf>, file: Option<PathBuf>, single: bool, man: bool) -> ExitCode {
    let size = if single { [860.0, 760.0] } else { [1100.0, 760.0] };
    let options = eframe::NativeOptions {
        viewport: patina::frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("Reader")
                .with_app_id("com.sectorflabs.Reader")
                .with_inner_size(size)
                .with_min_inner_size([480.0, 320.0]),
            true,
        ),
        ..Default::default()
    };
    match eframe::run_native(
        "reader",
        options,
        Box::new(move |cc| Ok(Box::new(app::Reader::new(cc, root, file, single, man)))),
    ) {
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

    fn run(args: &[&str]) -> Args {
        match parse(args.iter().map(|a| a.to_string())) {
            Ok(Parsed::Run(args)) => args,
            _ => panic!("{args:?} didn't parse"),
        }
    }

    #[test]
    fn reads_the_options() {
        assert_eq!(run(&["-s", "/a/b.md"]), Args { path: Some("/a/b.md".into()), single: true, ..Args::default() });
        assert_eq!(run(&["--file", "/a/b.md"]).file, Some("/a/b.md".into()));
        assert_eq!(run(&["--file=/a/b.md", "--single"]).file, Some("/a/b.md".into()));
        assert_eq!(run(&["/p", "--file", "/p/x.rs"]).path, Some("/p".into()));
        assert!(parse(["--file".to_string()]).is_err());
        assert!(parse(["--nope".to_string()]).is_err());
        assert!(parse(["/a".to_string(), "/b".to_string()]).is_err());
        assert!(matches!(parse(["-h".to_string()]), Ok(Parsed::Help)));
        let man = run(&["--man", "printf(3)"]);
        assert!(man.man && man.page.as_deref() == Some("printf(3)") && man.path.is_none());
        assert!(run(&["--man"]).page.is_none());
    }

    #[test]
    fn a_folder_and_a_file_open_the_file_in_the_folder() {
        let dir = std::env::temp_dir().join(format!("reader-args-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/a.md"), "").unwrap();
        let both = Args { path: Some(dir.clone()), file: Some(dir.join("docs/a.md")), ..Args::default() };
        assert_eq!(resolve(&both).unwrap(), (Some(dir.clone()), Some(dir.join("docs/a.md"))));
        let file_only = Args { file: Some(dir.join("docs/a.md")), ..Args::default() };
        assert_eq!(resolve(&file_only).unwrap().1, Some(dir.join("docs/a.md")));
        let missing = Args { file: Some(dir.join("nope.md")), ..Args::default() };
        assert!(resolve(&missing).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

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
