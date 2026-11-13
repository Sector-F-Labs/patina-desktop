//! Finding the files: fd for names and ripgrep for what's in them, run in
//! each folder that's on, on the caller's thread, and read as they come so a
//! run stops once it has enough. A run that takes too long is killed: a
//! search that's slow is one that's no longer wanted.

use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::{self, Config, Folder};

/// What's looked for at least this many characters at a time: one letter
/// matches nearly everything.
pub const LEAST: usize = 2;
/// The most lines read from one run.
const PER_RUN: usize = 20;
/// The most files shown.
pub const MOST: usize = 12;
/// How long a run may take before it's killed.
const DEADLINE: Duration = Duration::from_millis(1500);
/// Content searches skip files bigger than this.
const BIGGEST: &str = "2M";

/// One file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// The file's name.
    pub name: String,
    /// Where it is, as `~/Documents`.
    pub dir: String,
    /// The line it matched on and its text, for a hit in the contents.
    pub line: Option<(u64, String)>,
}

impl Hit {
    fn new(path: PathBuf, line: Option<(u64, String)>) -> Self {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        let dir = path.parent().map(config::short).unwrap_or_default();
        Hit {
            path,
            name,
            dir,
            line,
        }
    }
}

/// Where fd and ripgrep are, if they are.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub fd: Option<PathBuf>,
    pub rg: Option<PathBuf>,
}

impl Tools {
    /// Looks in `$PATH`: fd is `fdfind` on Debian and Ubuntu.
    pub fn find() -> Self {
        Tools {
            fd: on_path("fd").or_else(|| on_path("fdfind")),
            rg: on_path("rg"),
        }
    }
}

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?
        .to_string_lossy()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(program))
        .find(|p| p.is_file())
}

/// fd's arguments for files and folders in `dir` with `query` in their
/// names: not as a pattern, so what's typed is what's looked for.
pub fn fd_args(query: &str, dir: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--color",
        "never",
        "--fixed-strings",
        "--max-results",
        &PER_RUN.to_string(),
        "--",
        query,
    ]
    .map(OsString::from)
    .into();
    args.push(dir.into());
    args
}

/// ripgrep's arguments for the files in `dir` with `query` in them, one
/// line from each, its path then a NUL then `line:text`.
pub fn rg_args(query: &str, dir: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--color",
        "never",
        "--null",
        "--with-filename",
        "--line-number",
        "--no-heading",
        "--no-messages",
        "--fixed-strings",
        "--smart-case",
        "--max-count",
        "1",
        "--max-columns",
        "300",
        "--max-filesize",
        BIGGEST,
        "--",
        query,
    ]
    .map(OsString::from)
    .into();
    args.push(dir.into());
    args
}

/// A line of ripgrep's output as `rg_args` has it: the path, and the line
/// number and text of the match.
pub fn parse_rg(line: &[u8]) -> Option<(PathBuf, u64, String)> {
    let at = line.iter().position(|&b| b == 0)?;
    let (path, rest) = (&line[..at], &line[at + 1..]);
    let colon = rest.iter().position(|&b| b == b':')?;
    let number = std::str::from_utf8(&rest[..colon]).ok()?.parse().ok()?;
    let text = String::from_utf8_lossy(&rest[colon + 1..])
        .trim()
        .to_string();
    let path = PathBuf::from(OsString::from(String::from_utf8_lossy(path).into_owned()));
    Some((path, number, text))
}

/// Up to `limit` lines of what `program` prints, read as they come; then it
/// is stopped, as it is after `DEADLINE`.
fn run(program: &Path, args: &[OsString], limit: usize) -> Vec<Vec<u8>> {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let done = Arc::new(AtomicBool::new(false));
    let (pid, flag) = (child.id() as i32, done.clone());
    std::thread::spawn(move || {
        std::thread::sleep(DEADLINE);
        if !flag.load(Ordering::SeqCst) {
            // SAFETY: a signal to a child of ours that hasn't been waited for yet.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    });
    let mut lines = Vec::new();
    if let Some(out) = child.stdout.take() {
        let mut out = BufReader::new(out);
        while lines.len() < limit {
            let mut line = Vec::new();
            match out.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while line.last() == Some(&b'\n') {
                        line.pop();
                    }
                    if !line.is_empty() {
                        lines.push(line);
                    }
                }
            }
        }
    }
    done.store(true, Ordering::SeqCst);
    let _ = child.kill();
    let _ = child.wait();
    lines
}

/// Files in `folder` by name, as fd finds them.
fn by_name(tools: &Tools, folder: &Folder, query: &str) -> Vec<Hit> {
    let Some(fd) = &tools.fd else {
        return Vec::new();
    };
    run(fd, &fd_args(query, &folder.dir()), PER_RUN)
        .into_iter()
        .map(|l| {
            Hit::new(
                PathBuf::from(String::from_utf8_lossy(&l).into_owned()),
                None,
            )
        })
        .collect()
}

/// Files in `folder` with `query` in them, as ripgrep finds them.
fn by_contents(tools: &Tools, folder: &Folder, query: &str) -> Vec<Hit> {
    let Some(rg) = &tools.rg else {
        return Vec::new();
    };
    run(rg, &rg_args(query, &folder.dir()), PER_RUN)
        .iter()
        .filter_map(|l| parse_rg(l))
        .map(|(path, n, text)| Hit::new(path, Some((n, text))))
        .collect()
}

/// The files that match `query`: by name first, the ones that begin with it
/// before the others, then by contents, each file once; at most `MOST`.
/// Each folder and each tool runs at once.
pub fn search(query: &str, config: &Config, tools: &Tools) -> Vec<Hit> {
    let query = query.trim();
    if !config.enabled || query.chars().count() < LEAST {
        return Vec::new();
    }
    let folders: Vec<&Folder> = config.active().collect();
    let (names, contents) = std::thread::scope(|scope| {
        let named: Vec<_> = folders
            .iter()
            .filter(|f| f.style.names())
            .map(|f| scope.spawn(|| by_name(tools, f, query)))
            .collect();
        let inside: Vec<_> = folders
            .iter()
            .filter(|f| f.style.contents())
            .map(|f| scope.spawn(|| by_contents(tools, f, query)))
            .collect();
        let join = |handles: Vec<std::thread::ScopedJoinHandle<'_, Vec<Hit>>>| {
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_default())
                .collect::<Vec<_>>()
        };
        (join(named), join(inside))
    });
    combine(query, names, contents)
}

/// Name hits then content hits, the best names first, each file once.
fn combine(query: &str, mut names: Vec<Hit>, contents: Vec<Hit>) -> Vec<Hit> {
    let lower = query.to_lowercase();
    // Stable: fd's order is kept among names that rank the same.
    names.sort_by_key(|h| {
        let name = h.name.to_lowercase();
        if name == lower {
            0
        } else if name.starts_with(&lower) {
            1
        } else {
            2
        }
    });
    let mut seen = std::collections::HashSet::new();
    names
        .into_iter()
        .chain(contents)
        .filter(|h| seen.insert(h.path.clone()))
        .take(MOST)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Style;

    fn hit(path: &str) -> Hit {
        Hit::new(PathBuf::from(path), None)
    }

    #[test]
    fn a_query_is_looked_for_as_typed() {
        let fd = fd_args("a.b -c", Path::new("/x"));
        let end: Vec<_> = fd.iter().rev().take(3).collect();
        assert_eq!(
            end,
            ["/x", "a.b -c", "--"],
            "after --, so a dash is no option"
        );
        assert!(fd.contains(&"--fixed-strings".into()));
        let rg = rg_args("(oops", Path::new("/x"));
        assert!(rg.contains(&"--fixed-strings".into()));
        assert!(rg.contains(&"--smart-case".into()));
        assert_eq!(rg.last(), Some(&OsString::from("/x")));
    }

    #[test]
    fn a_line_of_ripgrep_is_split_by_its_nul() {
        let line = b"/home/me/notes/a: b.md\x0012:  the text, with: colons ";
        assert_eq!(
            parse_rg(line),
            Some((
                PathBuf::from("/home/me/notes/a: b.md"),
                12,
                "the text, with: colons".to_string()
            ))
        );
        assert_eq!(parse_rg(b"no nul here"), None);
        assert_eq!(parse_rg(b"/p\x00x:text"), None, "a line number is a number");
    }

    #[test]
    fn names_that_begin_with_it_come_first_then_contents_each_file_once() {
        let names = vec![
            hit("/a/old-report.txt"),
            hit("/a/report.txt"),
            hit("/a/Report"),
        ];
        let mut inside = Hit::new(PathBuf::from("/a/notes.md"), Some((3, "a report".into())));
        let again = Hit::new(PathBuf::from("/a/report.txt"), Some((1, "report".into())));
        inside.line = Some((3, "a report".into()));
        let all = combine("report", names, vec![again, inside]);
        let paths: Vec<_> = all.iter().map(|h| h.path.to_str().unwrap()).collect();
        assert_eq!(
            paths,
            [
                "/a/Report",
                "/a/report.txt",
                "/a/old-report.txt",
                "/a/notes.md"
            ]
        );
    }

    #[test]
    fn a_short_query_or_search_turned_off_finds_nothing() {
        let tools = Tools::default();
        let mut config = Config::default();
        assert!(search("a", &config, &tools).is_empty());
        config.enabled = false;
        assert!(search("abc", &config, &tools).is_empty());
    }

    #[test]
    fn a_run_stops_at_its_limit() {
        let lines = run(Path::new("/bin/sh"), &["-c".into(), "yes line".into()], 3);
        assert_eq!(lines, vec![b"line".to_vec(); 3]);
        assert!(run(Path::new("/no/such/program"), &[], 3).is_empty());
    }

    /// With fd and ripgrep installed, finds files by name and by contents in
    /// a folder of its own.
    #[test]
    fn fd_and_ripgrep_find_files_when_they_are_installed() {
        let tools = Tools::find();
        if tools.fd.is_none() || tools.rg.is_none() {
            eprintln!("fd or ripgrep isn't installed; skipped");
            return;
        }
        let dir = std::env::temp_dir().join(format!("patina-search-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("quarterly-budget.txt"), "numbers\n").unwrap();
        std::fs::write(
            dir.join("minutes.md"),
            "we discussed the budget\nand more\n",
        )
        .unwrap();
        std::fs::write(dir.join("other.md"), "nothing here\n").unwrap();
        let config = Config {
            enabled: true,
            folders: vec![Folder::new(dir.to_string_lossy(), Style::Both)],
        };
        let found = search("budget", &config, &tools);
        let names: Vec<_> = found.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["quarterly-budget.txt", "minutes.md"], "{found:?}");
        assert_eq!(found[1].line, Some((1, "we discussed the budget".into())));
        let by_name_only = Config {
            folders: vec![Folder::new(dir.to_string_lossy(), Style::Names)],
            ..config.clone()
        };
        assert_eq!(search("budget", &by_name_only, &tools).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
