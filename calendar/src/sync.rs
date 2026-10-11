//! Talking to the server off the UI thread. The app sends `Command`s; a
//! worker thread runs them with the CalDAV client and sends back `Update`s,
//! asking egui to repaint. Fetched data is cached on disk so the app opens
//! with the last known calendars, even offline (ADR-0003).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use serde::{Deserialize, Serialize};

use crate::caldav::{Account, Client, Object};
use crate::model::Calendar;

/// A stored calendar object: which calendar, its href, etag and text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stored {
    pub calendar: String,
    pub href: String,
    pub etag: Option<String>,
    pub ics: String,
}

/// Everything fetched from the server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub calendars: Vec<CachedCalendar>,
    pub objects: Vec<Stored>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedCalendar {
    pub href: String,
    pub name: String,
    pub color: Option<String>,
    pub ctag: String,
}

impl From<&Calendar> for CachedCalendar {
    fn from(c: &Calendar) -> Self {
        Self { href: c.href.clone(), name: c.name.clone(), color: c.color.clone(), ctag: c.ctag.clone() }
    }
}

impl From<&CachedCalendar> for Calendar {
    fn from(c: &CachedCalendar) -> Self {
        Self { href: c.href.clone(), name: c.name.clone(), color: c.color.clone(), ctag: c.ctag.clone() }
    }
}

pub enum Command {
    SetAccount(Account),
    Refresh,
    /// Writes an event to `href`. With `replaces`, the event moved from
    /// another calendar and the old file is deleted after the write.
    Save {
        href: String,
        ics: String,
        etag: Option<String>,
        replaces: Option<(String, Option<String>)>,
    },
    Delete {
        href: String,
        etag: Option<String>,
    },
    SetColor {
        calendar: String,
        color: String,
    },
}

pub enum Update {
    Busy(bool),
    Synced(Snapshot),
    Error(String),
}

pub struct Sync {
    tx: Sender<Command>,
    rx: Receiver<Update>,
    pub busy: bool,
}

impl Sync {
    pub fn start(ctx: &egui::Context, account: Account, cached: Snapshot) -> Self {
        let (tx, commands) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::Builder::new()
            .name("caldav".into())
            .spawn(move || worker(commands, updates, ctx, account, cached))
            .expect("start the sync thread");
        Self { tx, rx, busy: false }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    /// Updates that arrived since the last call.
    pub fn poll(&mut self) -> Vec<Update> {
        let updates: Vec<Update> = self.rx.try_iter().collect();
        for u in &updates {
            if let Update::Busy(b) = u {
                self.busy = *b;
            }
        }
        updates
    }
}

fn worker(
    commands: Receiver<Command>,
    updates: Sender<Update>,
    ctx: egui::Context,
    mut account: Account,
    mut snapshot: Snapshot,
) {
    let send = |u: Update| {
        let _ = updates.send(u);
        ctx.request_repaint();
    };
    let mut client = Client::new(account.clone()).ok();
    while let Ok(first) = commands.recv() {
        // Run everything queued, then refresh once.
        let mut queue = vec![first];
        queue.extend(commands.try_iter());
        send(Update::Busy(true));
        for command in queue {
            let result = match command {
                Command::SetAccount(a) => {
                    account = a;
                    snapshot = Snapshot::default();
                    client = Client::new(account.clone()).map_err(|e| send(Update::Error(e))).ok();
                    Ok(())
                }
                Command::Refresh => Ok(()),
                Command::Save { href, ics, etag, replaces } => with(&client, |c| {
                    c.put(&href, &ics, etag.as_deref())?;
                    if let Some((old, old_etag)) = replaces {
                        c.delete(&old, old_etag.as_deref())?;
                    }
                    Ok(())
                }),
                Command::Delete { href, etag } => with(&client, |c| c.delete(&href, etag.as_deref())),
                Command::SetColor { calendar, color } => with(&client, |c| c.set_color(&calendar, &color)),
            };
            if let Err(e) = result {
                send(Update::Error(e));
            }
        }
        // Every batch ends with a refresh, so edits show what the server stored.
        if account.is_set() {
            match with(&client, |c| fetch(c, &snapshot)) {
                Ok(fresh) => {
                    snapshot = fresh;
                    save_cache(&snapshot);
                    send(Update::Synced(snapshot.clone()));
                }
                Err(e) => send(Update::Error(e)),
            }
        }
        send(Update::Busy(false));
    }
}

fn with<T>(client: &Option<Client>, f: impl FnOnce(&Client) -> Result<T, String>) -> Result<T, String> {
    match client {
        Some(c) => f(c),
        None => Err("Set up the server in File → Account…".into()),
    }
}

/// Fetches the calendars, and the events of those whose ctag changed.
fn fetch(client: &Client, old: &Snapshot) -> Result<Snapshot, String> {
    let calendars = client.calendars()?;
    let mut objects = Vec::new();
    for cal in &calendars {
        let unchanged = !cal.ctag.is_empty() && old.calendars.iter().any(|c| c.href == cal.href && c.ctag == cal.ctag);
        if unchanged {
            objects.extend(old.objects.iter().filter(|o| o.calendar == cal.href).cloned());
            continue;
        }
        for Object { href, etag, ics } in client.events(&cal.href)? {
            objects.push(Stored { calendar: cal.href.clone(), href, etag, ics });
        }
    }
    Ok(Snapshot { calendars: calendars.iter().map(CachedCalendar::from).collect(), objects })
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("calendar").join("snapshot.json"))
}

pub fn load_cache() -> Snapshot {
    cache_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_cache(snapshot: &Snapshot) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(snapshot) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}
