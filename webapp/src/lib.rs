//! Web apps (ADR-0133): a URL kept in a list and installed as an app. Adding one writes a
//! `.desktop` entry, so the launcher, the bar and Alt+Tab list it like any app; removing one
//! takes the entry and the app's profile away. Launching starts a browser engine, Chromium's
//! `--app` mode, in a window with no browser chrome and a profile of its own, under an app-id
//! whose app-id ends in `-patina-webapp-<slug>` (see [`slug_from_app_id`]), so the compositor
//! can tell it apart.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod look;
pub mod meta;

/// What every web app's app-id and `.desktop` file name starts with.
pub const ID_PREFIX: &str = "patina-webapp-";

/// Engines tried in order, when `PATINA_WEBAPP_ENGINE` doesn't name one.
const ENGINES: [&str; 6] = [
    "chromium",
    "chromium-browser",
    "google-chrome-stable",
    "google-chrome",
    "brave-browser",
    "/snap/bin/chromium",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebApp {
    pub slug: String,
    pub name: String,
    pub url: String,
    /// What opens it; WebKit when unset.
    #[serde(default, skip_serializing_if = "is_webkit")]
    pub engine: Engine,
    /// The site's theme colour, `#rrggbb`: the window's frame is this (read on install and
    /// refresh, never at launch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Its icon, a file in the app's folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// What a web app's page runs in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// `patina-webview`, on WebKitGTK: the window is the page, and the desktop's.
    #[default]
    WebKit,
    /// Chromium's `--app` mode: more sites work (DRM, some calls), at the cost of a second
    /// browser installed.
    Chromium,
}

fn is_webkit(e: &Engine) -> bool {
    *e == Engine::WebKit
}

impl WebApp {
    /// The name of the app's profile inside its user-data directory. Chromium on Wayland takes
    /// its windows' app-id from the address and this (`--class` does nothing there), so it's
    /// where the slug is carried.
    pub fn profile_name(&self) -> String {
        format!("{ID_PREFIX}{}", self.slug)
    }

    /// The app-id its window has: the WebKit host's own, or Chromium's
    /// `chrome-<host>_<path>-<profile>`, which is only known for an address with no path; a
    /// longer one gets none, and matches by [`slug_from_app_id`].
    pub fn app_id(&self) -> Option<String> {
        if self.engine == Engine::WebKit {
            return Some(self.profile_name());
        }
        let rest = self.url.split("://").nth(1)?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let simple = path.is_empty() && !host.contains(['?', '#', ':']);
        simple.then(|| format!("chrome-{host}__-{}", self.profile_name()))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default, rename = "app")]
    apps: Vec<WebApp>,
}

/// Where the list, the profiles and the `.desktop` entries live.
#[derive(Debug, Clone)]
pub struct Store {
    config: PathBuf,
    data: PathBuf,
    /// `~/snap`, where snap engines keep their data.
    snap_home: Option<PathBuf>,
}

impl Store {
    /// The user's: `~/.config/patina-webapp` and `~/.local/share`.
    pub fn user() -> io::Result<Self> {
        let config = dirs::config_dir().ok_or_else(|| io::Error::other("no config directory"))?;
        let data = dirs::data_dir().ok_or_else(|| io::Error::other("no data directory"))?;
        let mut store = Self::at(config, data);
        store.snap_home = dirs::home_dir().map(|h| h.join("snap"));
        Ok(store)
    }

    pub fn at(config: PathBuf, data: PathBuf) -> Self {
        Self {
            config,
            data,
            snap_home: None,
        }
    }

    /// The file the list is kept in; its modification time is when it last changed.
    pub fn list_path(&self) -> PathBuf {
        self.config.join("patina-webapp").join("apps.toml")
    }

    /// The app's browser profile: its cookies and logins, apart from every other web app's.
    pub fn profile_dir(&self, slug: &str) -> PathBuf {
        self.data.join("patina-webapp").join(slug)
    }

    /// Where an engine keeps the app's profile: [`Store::profile_dir`], or for a snap's (confined
    /// to `~/snap/<name>/`, and out of every hidden folder) its own `common` folder.
    pub fn profile_dir_for(&self, slug: &str, engine: &Path) -> PathBuf {
        match (snap_name(engine), &self.snap_home) {
            (Some(name), Some(home)) => home
                .join(name)
                .join("common")
                .join("patina-webapp")
                .join(slug),
            _ => self.profile_dir(slug),
        }
    }

    pub fn desktop_path(&self, slug: &str) -> PathBuf {
        self.data
            .join("applications")
            .join(format!("{ID_PREFIX}{slug}.desktop"))
    }

    /// The installed web apps, in the order they were added.
    pub fn list(&self) -> Vec<WebApp> {
        fs::read_to_string(self.list_path())
            .ok()
            .and_then(|text| toml::from_str::<File>(&text).ok())
            .map(|f| f.apps)
            .unwrap_or_default()
    }

    pub fn find(&self, slug: &str) -> Option<WebApp> {
        self.list().into_iter().find(|a| a.slug == slug)
    }

    fn save(&self, apps: Vec<WebApp>) -> io::Result<()> {
        let path = self.list_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(&File { apps }).map_err(io::Error::other)?;
        fs::write(path, text)
    }

    /// Installs a web app: the URL (`https://` is assumed) and a name, the site's if empty.
    pub fn add(&self, name: &str, url: &str) -> Result<WebApp, String> {
        let url = normalise_url(url)?;
        let name = match name.trim() {
            "" => default_name(&url),
            n => n.to_string(),
        };
        let mut apps = self.list();
        let same_site = |a: &str| a.trim_end_matches('/') == url.trim_end_matches('/');
        if let Some(same) = apps.iter().find(|a| same_site(&a.url)) {
            return Err(format!("{} already opens {url}", same.name));
        }
        let base = slug_for(&name);
        let mut slug = base.clone();
        let mut n = 2;
        while apps.iter().any(|a| a.slug == slug) {
            slug = format!("{base}-{n}");
            n += 1;
        }
        let app = WebApp {
            slug,
            name,
            url,
            engine: Engine::default(),
            color: None,
            icon: None,
        };
        apps.push(app.clone());
        self.save(apps).map_err(|e| e.to_string())?;
        self.write_entry(&app).map_err(|e| e.to_string())?;
        Ok(app)
    }

    /// Removes a web app, its `.desktop` entry and its profile (its logins with it).
    pub fn remove(&self, slug: &str) -> Result<(), String> {
        let mut apps = self.list();
        let before = apps.len();
        let removed = apps.iter().find(|a| a.slug == slug).cloned();
        apps.retain(|a| a.slug != slug);
        if apps.len() == before {
            return Err(format!("no web app called {slug}"));
        }
        self.save(apps).map_err(|e| e.to_string())?;
        ignore_missing(fs::remove_file(self.desktop_path(slug))).map_err(|e| e.to_string())?;
        if let Some(icon) = removed.and_then(|a| a.icon) {
            ignore_missing(fs::remove_file(icon)).map_err(|e| e.to_string())?;
        }
        let mut profiles = vec![self.profile_dir(slug)];
        for engine in ENGINES.iter().filter(|e| snap_name(Path::new(e)).is_some()) {
            profiles.push(self.profile_dir_for(slug, Path::new(engine)));
        }
        profiles
            .iter()
            .try_for_each(|p| ignore_missing(fs::remove_dir_all(p)))
            .map_err(|e| e.to_string())
    }

    /// Keeps `color` (`#rrggbb`) as the web app's: what its page shows now, which the running
    /// host reports.
    pub fn set_color(&self, slug: &str, color: &str) -> Result<(), String> {
        let mut apps = self.list();
        let app = apps
            .iter_mut()
            .find(|a| a.slug == slug)
            .ok_or(format!("no web app called {slug}"))?;
        if app.color.as_deref() == Some(color) {
            return Ok(());
        }
        app.color = Some(color.to_string());
        self.save(apps).map_err(|e| e.to_string())
    }

    /// Reads the site again for its colour and icon, and keeps what it says (what it doesn't
    /// say is kept as it was). Slow: it's the network, so not on a UI thread.
    pub fn refresh(&self, slug: &str) -> Result<WebApp, String> {
        let mut app = self.find(slug).ok_or(format!("no web app called {slug}"))?;
        let found = meta::read(&app.url);
        if found.color.is_none() && found.icon.is_none() {
            return Err(format!("{} told nothing about itself (offline?)", app.url));
        }
        if found.color.is_some() {
            app.color = found.color;
        }
        if let Some(icon) = found.icon
            && let Some(path) =
                meta::download_icon(&icon, &self.data.join("patina-webapp").join("icons"), slug)
        {
            // An icon from before icons were PNGs is left behind otherwise.
            if let Some(old) = app.icon.as_deref().filter(|old| Path::new(old) != path) {
                let _ = fs::remove_file(old);
            }
            app.icon = Some(path.display().to_string());
        }
        let mut apps = self.list();
        if let Some(slot) = apps.iter_mut().find(|a| a.slug == slug) {
            *slot = app.clone();
        }
        self.save(apps).map_err(|e| e.to_string())?;
        self.write_entry(&app).map_err(|e| e.to_string())?;
        Ok(app)
    }

    /// Writes every app's `.desktop` entry again, for a changed `patina-webapp` path.
    pub fn write_entries(&self) -> io::Result<()> {
        self.list().iter().try_for_each(|a| self.write_entry(a))
    }

    fn write_entry(&self, app: &WebApp) -> io::Result<()> {
        let path = self.desktop_path(&app.slug);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, desktop_entry(app))
    }

    /// The command that opens the app: the engine, in `--app` mode, with its own profile.
    pub fn command(&self, app: &WebApp, engine: &Path) -> Vec<String> {
        vec![
            engine.display().to_string(),
            "--ozone-platform=wayland".into(),
            format!(
                "--user-data-dir={}",
                self.profile_dir_for(&app.slug, engine).display()
            ),
            format!("--profile-directory={}", app.profile_name()),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            format!("--app={}", app.url),
        ]
    }
}

/// The snap an engine is, from its `/snap/bin/<name>` path.
fn snap_name(engine: &Path) -> Option<&str> {
    let rest = engine.strip_prefix("/snap/bin").ok()?;
    rest.to_str().filter(|n| !n.contains('/'))
}

fn ignore_missing(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// The web app a window's app-id belongs to: `patina-webapp-whatsapp` and Chromium's
/// `chrome-web.whatsapp.com__-patina-webapp-whatsapp` are both `whatsapp`.
pub fn slug_from_app_id(app_id: &str) -> Option<&str> {
    let slug = match app_id.strip_prefix(ID_PREFIX) {
        Some(slug) => slug,
        None => app_id.rsplit_once(concat!("-", "patina-webapp-"))?.1,
    };
    (!slug.is_empty()).then_some(slug)
}

/// The first engine installed: `PATINA_WEBAPP_ENGINE` if set, else a Chromium on the PATH.
pub fn find_engine() -> Option<PathBuf> {
    if let Some(engine) = std::env::var_os("PATINA_WEBAPP_ENGINE") {
        return Some(PathBuf::from(engine));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    ENGINES.iter().find_map(|name| {
        let name = Path::new(name);
        if name.is_absolute() {
            return name.is_file().then(|| name.to_path_buf());
        }
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    })
}

/// The WebKit host: `patina-webview` beside the running binary (the install, or `target/`),
/// else on the PATH.
pub fn find_webview() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("patina-webview")))
        .filter(|p| p.is_file());
    beside.or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join("patina-webview"))
            .find(|p| p.is_file())
    })
}

/// Whether `app` can be opened here: its engine is installed.
pub fn can_open(app: &WebApp) -> bool {
    match app.engine {
        Engine::WebKit => find_webview().is_some(),
        Engine::Chromium => find_engine().is_some(),
    }
}

/// A URL the way a person types it: `web.whatsapp.com` gets `https://`.
pub fn normalise_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Type the address of the web app".into());
    }
    let url = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| "Only http and https addresses open as web apps".to_string())?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    if name.is_empty() || (!name.contains('.') && name != "localhost") {
        return Err(format!("{input} isn't an address"));
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("An address has no spaces in it".into());
    }
    Ok(url)
}

/// A name for a site that was given none: `web.whatsapp.com` is Whatsapp.
pub fn default_name(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#', ':'])
        .next()
        .unwrap_or("");
    let parts: Vec<&str> = host.split('.').collect();
    // The label before the top-level domain: web.whatsapp.com → whatsapp.
    let label = match parts.len() {
        0 | 1 => host,
        n => parts[n - 2],
    };
    let mut chars = label.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => "Web app".into(),
    }
}

/// A name as a file name and app-id part: `Google Meet` is `google-meet`.
pub fn slug_for(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "app".into()
    } else {
        slug.into()
    }
}

fn desktop_entry(app: &WebApp) -> String {
    // A name is one line; a control character would start another key.
    let name: String = app.name.chars().filter(|c| !c.is_control()).collect();
    let class = app
        .app_id()
        .map(|id| format!("StartupWMClass={id}\n"))
        .unwrap_or_default();
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Comment={url}\n\
         Exec=patina-webapp launch {slug}\n\
         Icon={icon}\n\
         Categories=Network;\n\
         {class}\
         StartupNotify=true\n\
         X-Patina-WebApp={url}\n",
        url = app.url,
        icon = app.icon.as_deref().unwrap_or("applications-internet"),
        slug = app.slug,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "patina-webapp-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        (Store::at(dir.join("config"), dir.join("data")), dir)
    }

    #[test]
    fn urls_are_completed_and_checked() {
        assert_eq!(
            normalise_url(" web.whatsapp.com ").unwrap(),
            "https://web.whatsapp.com"
        );
        assert_eq!(
            normalise_url("http://localhost:8080/x").unwrap(),
            "http://localhost:8080/x"
        );
        assert!(normalise_url("").is_err());
        assert!(normalise_url("whatsapp").is_err());
        assert!(normalise_url("file:///etc/passwd").is_err());
        assert!(normalise_url("https://a b.com").is_err());
    }

    #[test]
    fn names_and_slugs() {
        assert_eq!(default_name("https://web.whatsapp.com/"), "Whatsapp");
        assert_eq!(default_name("https://meet.google.com"), "Google");
        assert_eq!(slug_for("Google Meet!"), "google-meet");
        assert_eq!(slug_for("???"), "app");
    }

    #[test]
    fn add_installs_an_entry_and_remove_takes_it_away() {
        let (s, dir) = store();
        let a = s.add("", "web.whatsapp.com").unwrap();
        assert_eq!((a.slug.as_str(), a.name.as_str()), ("whatsapp", "Whatsapp"));
        assert_eq!(a.engine, Engine::WebKit);
        let entry = fs::read_to_string(s.desktop_path("whatsapp")).unwrap();
        assert!(entry.contains("Exec=patina-webapp launch whatsapp"));
        assert!(entry.contains("StartupWMClass=patina-webapp-whatsapp"));
        assert_eq!(
            slug_from_app_id("chrome-web.whatsapp.com__-patina-webapp-whatsapp"),
            Some("whatsapp")
        );
        assert_eq!(slug_from_app_id("patina-webapp-whatsapp"), Some("whatsapp"));
        assert_eq!(slug_from_app_id("chrome-x__-Default"), None);
        // The same address twice is refused; the same name gets a suffix.
        assert!(s.add("Other", "https://web.whatsapp.com").is_err());
        assert!(
            s.add("Other", "https://web.whatsapp.com/").is_err(),
            "a slash is the same site"
        );
        assert_eq!(
            s.add("Whatsapp", "https://whatsapp.com").unwrap().slug,
            "whatsapp-2"
        );
        assert_eq!(s.list().len(), 2);

        fs::create_dir_all(s.profile_dir("whatsapp")).unwrap();
        s.remove("whatsapp").unwrap();
        assert!(!s.desktop_path("whatsapp").exists());
        assert!(!s.profile_dir("whatsapp").exists());
        assert_eq!(s.list().len(), 1);
        assert!(s.remove("whatsapp").is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_snap_engine_keeps_its_profile_in_its_own_folder() {
        let mut s = Store::at("/c".into(), "/home/u/.local/share".into());
        s.snap_home = Some("/home/u/snap".into());
        assert_eq!(
            s.profile_dir_for("x", Path::new("/snap/bin/chromium")),
            Path::new("/home/u/snap/chromium/common/patina-webapp/x")
        );
        assert_eq!(
            s.profile_dir_for("x", Path::new("/usr/bin/chromium")),
            Path::new("/home/u/.local/share/patina-webapp/x")
        );
    }

    #[test]
    fn the_command_is_app_mode_with_its_own_profile() {
        let (s, _) = store();
        let app = WebApp {
            slug: "x".into(),
            name: "X".into(),
            url: "https://x.com".into(),
            engine: Engine::Chromium,
            color: None,
            icon: None,
        };
        let cmd = s.command(&app, Path::new("/usr/bin/chromium"));
        assert!(cmd.contains(&"--app=https://x.com".to_string()));
        assert!(cmd.contains(&"--profile-directory=patina-webapp-x".to_string()));
        assert!(
            cmd.iter()
                .any(|a| a.starts_with("--user-data-dir=") && a.ends_with("/x"))
        );
    }
}
