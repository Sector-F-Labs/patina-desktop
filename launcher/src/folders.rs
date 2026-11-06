//! Apps that are folders: `~/Apps/Name.app`, each with an `app.toml` saying
//! how to run it (ADR-0035, and Patina OS's ADR-0004 for the format). They're
//! read by patina-apps, which Files shares, and listed beside the .desktop
//! entries. For now each runs straight on the running system; Patina OS's
//! runner will give it the base it names.

use crate::apps::App;

/// The folder apps in `~/Apps`; ones that can't be run are left out, with
/// the reason on stderr.
pub fn load() -> Vec<App> {
    let Some(dir) = patina_apps::dir() else {
        return Vec::new();
    };
    patina_apps::list(&dir)
        .into_iter()
        .filter_map(|(folder, app)| match app {
            Ok(app) => Some(entry(app)),
            Err(e) => {
                eprintln!("patina-launcher: {}: {e}", folder.display());
                None
            }
        })
        .collect()
}

/// A folder app as the launcher lists and starts it, from the home folder.
fn entry(app: patina_apps::App) -> App {
    let mut exec = vec![app.program.to_string_lossy().into_owned()];
    exec.extend(app.args);
    App {
        id: app.id,
        name: app.name,
        generic: app.summary,
        comment: app.comment,
        keywords: app.keywords,
        exec,
        terminal: app.terminal,
        path: std::env::var("HOME").ok(),
        icon: app.icon.map(|i| i.to_string_lossy().into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_app_runs_its_program_from_home() {
        let app = entry(patina_apps::App {
            folder: "/a/T.app".into(),
            id: "T".into(),
            name: "Teleprinter".into(),
            summary: Some("Terminal".into()),
            comment: None,
            keywords: vec!["shell".into()],
            categories: Vec::new(),
            icon: Some("/a/T.app/icon.svg".into()),
            program: "/a/T.app/bin/t".into(),
            args: vec!["--window".into()],
            terminal: false,
        });
        assert_eq!(app.exec, ["/a/T.app/bin/t", "--window"]);
        assert_eq!(app.generic.as_deref(), Some("Terminal"));
        assert_eq!(app.icon.as_deref(), Some("/a/T.app/icon.svg"));
        assert_eq!(app.path, std::env::var("HOME").ok());
    }
}
