//! Apps that are folders: `~/Apps/Name.app`, each with an `app.toml` saying
//! how to run it (ADR-0035, and Patina OS's ADR-0004 for the format). They're
//! listed beside the .desktop entries. For now each runs straight on the
//! running system; Patina OS's runner will give it the base it names.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::apps::App;

/// The app.toml formats this launcher understands.
const FORMAT: u32 = 1;

/// app.toml as written; keys this launcher doesn't know are ignored, so
/// optional ones can be added without a new format.
#[derive(Deserialize)]
struct Manifest {
    format: u32,
    name: String,
    exec: Exec,
    id: Option<String>,
    summary: Option<String>,
    comment: Option<String>,
    #[serde(default)]
    keywords: Vec<String>,
    icon: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    terminal: bool,
}

/// One program, or one for each platform (`linux-x86_64`, `freebsd-x86_64`).
#[derive(Deserialize)]
#[serde(untagged)]
enum Exec {
    One(String),
    PerPlatform(BTreeMap<String, String>),
}

/// This platform, as `[exec]` names them.
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `~/Apps`.
pub fn dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Apps"))
}

/// The folder apps in `dir`; ones that can't be run are left out, with the
/// reason on stderr.
pub fn load(dir: &Path) -> Vec<App> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| is_app_folder(p))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .filter_map(|folder| match read(&folder, &platform()) {
            Ok(app) => Some(app),
            Err(e) => {
                eprintln!("patina-launcher: {}: {e}", folder.display());
                None
            }
        })
        .collect()
}

/// A folder named `*.app`, not hidden.
fn is_app_folder(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    !name.starts_with('.') && name.ends_with(".app") && name.len() > 4 && path.is_dir()
}

/// The app in `folder`, for `platform`.
fn read(folder: &Path, platform: &str) -> Result<App, String> {
    let text =
        std::fs::read_to_string(folder.join("app.toml")).map_err(|e| format!("app.toml: {e}"))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|e| format!("app.toml: {e}"))?;
    if manifest.format != FORMAT {
        return Err(format!(
            "app.toml is format {}; this launcher reads format {FORMAT}",
            manifest.format
        ));
    }
    let program = match &manifest.exec {
        Exec::One(path) => path,
        Exec::PerPlatform(paths) => paths
            .get(platform)
            .ok_or_else(|| format!("no program for {platform}"))?,
    };
    let program = inside(folder, program).ok_or("exec must be a path inside the folder")?;
    if !executable(&program) {
        return Err(format!("{} isn't an executable file", program.display()));
    }
    // A missing or unsafe icon only loses the icon.
    let icon = manifest
        .icon
        .as_deref()
        .and_then(|i| inside(folder, i))
        .filter(|i| i.is_file())
        .map(|i| i.to_string_lossy().into_owned());
    let stem = folder
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".app"))
        .unwrap_or_default();
    let mut exec = vec![program.to_string_lossy().into_owned()];
    exec.extend(manifest.args);
    Ok(App {
        id: manifest.id.unwrap_or_else(|| stem.to_string()),
        name: manifest.name,
        generic: manifest.summary,
        comment: manifest.comment,
        keywords: manifest.keywords,
        exec,
        terminal: manifest.terminal,
        path: None,
        icon,
    })
}

/// `relative` within `folder`, or `None` if it's absolute or climbs out.
fn inside(folder: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    let plain = path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    (plain && !relative.is_empty()).then(|| folder.join(path))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of apps for one test, removed when it's dropped.
    struct Apps(PathBuf);

    impl Apps {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("patina-launcher-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Apps(dir)
        }

        /// `folder` with `toml` as its app.toml and executables at `programs`.
        fn add(&self, folder: &str, toml: &str, programs: &[&str]) -> PathBuf {
            use std::os::unix::fs::PermissionsExt;
            let folder = self.0.join(folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("app.toml"), toml).unwrap();
            for p in programs {
                let path = folder.join(p);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "#!/bin/sh\n").unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            folder
        }
    }

    impl Drop for Apps {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_an_app_folder() {
        let apps = Apps::new("reads");
        let folder = apps.add(
            "Teleprinter.app",
            "format = 1\nname = \"Teleprinter\"\nexec = \"bin/teleprinter\"\n\
             summary = \"Terminal\"\nkeywords = [\"shell\"]\nargs = [\"--window\"]\n\
             icon = \"icon.png\"\nbase = \"patina-os:1@sha256:00\"\nlater = \"ignored\"\n",
            &["bin/teleprinter"],
        );
        std::fs::write(folder.join("icon.png"), b"").unwrap();
        let app = read(&folder, "linux-x86_64").unwrap();
        assert_eq!(app.name, "Teleprinter");
        assert_eq!(app.id, "Teleprinter");
        assert_eq!(app.generic.as_deref(), Some("Terminal"));
        assert_eq!(app.keywords, ["shell"]);
        let program = folder
            .join("bin/teleprinter")
            .to_string_lossy()
            .into_owned();
        assert_eq!(app.exec, [program, "--window".to_string()]);
        let icon = folder.join("icon.png").to_string_lossy().into_owned();
        assert_eq!(app.icon, Some(icon));
    }

    #[test]
    fn picks_the_program_for_this_platform() {
        let apps = Apps::new("platform");
        let folder = apps.add(
            "Tool.app",
            "format = 1\nname = \"Tool\"\n[exec]\nlinux-x86_64 = \"bin/linux/tool\"\n\
             freebsd-x86_64 = \"bin/freebsd/tool\"\n",
            &["bin/linux/tool", "bin/freebsd/tool"],
        );
        let on = |p| read(&folder, p).map(|a| a.exec[0].clone());
        assert!(on("linux-x86_64").unwrap().ends_with("bin/linux/tool"));
        assert!(on("freebsd-x86_64").unwrap().ends_with("bin/freebsd/tool"));
        assert_eq!(
            on("linux-aarch64").unwrap_err(),
            "no program for linux-aarch64"
        );
    }

    #[test]
    fn refuses_what_it_cant_run_correctly() {
        let apps = Apps::new("refuses");
        let err = |folder: &str, toml: &str, programs: &[&str]| {
            read(&apps.add(folder, toml, programs), "linux-x86_64").unwrap_err()
        };
        assert!(
            err(
                "Out.app",
                "format = 1\nname = \"O\"\nexec = \"../x\"\n",
                &[]
            )
            .contains("inside")
        );
        assert!(
            err(
                "Abs.app",
                "format = 1\nname = \"A\"\nexec = \"/bin/sh\"\n",
                &[]
            )
            .contains("inside")
        );
        assert!(
            err(
                "Gone.app",
                "format = 1\nname = \"G\"\nexec = \"bin/g\"\n",
                &[]
            )
            .contains("executable")
        );
        assert!(
            err(
                "New.app",
                "format = 2\nname = \"N\"\nexec = \"n\"\n",
                &["n"]
            )
            .contains("format 2")
        );
        assert!(err("Bad.app", "name = \"B\"\n", &[]).starts_with("app.toml"));
    }

    #[test]
    fn an_unsafe_icon_is_only_dropped() {
        let apps = Apps::new("icon");
        let folder = apps.add(
            "I.app",
            "format = 1\nname = \"I\"\nexec = \"i\"\nicon = \"../../etc/x.png\"\n",
            &["i"],
        );
        assert_eq!(read(&folder, "linux-x86_64").unwrap().icon, None);
    }

    #[test]
    fn lists_only_app_folders() {
        let apps = Apps::new("lists");
        apps.add("B.app", "format = 1\nname = \"B\"\nexec = \"b\"\n", &["b"]);
        apps.add("A.app", "format = 1\nname = \"A\"\nexec = \"a\"\n", &["a"]);
        apps.add("notes", "format = 1\nname = \"N\"\nexec = \"n\"\n", &["n"]);
        apps.add(
            ".Hidden.app",
            "format = 1\nname = \"H\"\nexec = \"h\"\n",
            &["h"],
        );
        apps.add("Broken.app", "format = 1\n", &[]);
        let names: Vec<String> = load(&apps.0).into_iter().map(|a| a.name).collect();
        assert_eq!(names, ["A", "B"]);
        assert!(load(&apps.0.join("nowhere")).is_empty());
    }
}
