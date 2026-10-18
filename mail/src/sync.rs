//! Talking to Bridge off the UI thread (ADR-0002). The app sends `Command`s;
//! a worker thread runs them over one IMAP session and sends back
//! `Update`s, asking egui to repaint. Sending goes over SMTP on a thread of
//! its own, so a slow send never holds up reading.

use std::sync::mpsc::{self, Receiver, Sender};

use imap::{ClientBuilder, ConnectionMode, TlsKind};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{SmtpTransport, Transport};

use crate::config::{Account, Security};
use crate::message::{Body, Summary};

/// How many of a folder's newest messages the list shows.
pub const PAGE: u32 = 200;

type Session = imap::Session<imap::Connection>;

/// A folder in the sidebar.
#[derive(Debug, Clone, PartialEq)]
pub struct Folder {
    /// The full IMAP name, such as `Folders/Receipts`.
    pub name: String,
    /// The last part of the name, to show.
    pub label: String,
    /// How deep it is under other folders.
    pub depth: usize,
    pub unseen: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    Seen,
    Flagged,
}

impl Flag {
    fn imap(self) -> &'static str {
        match self {
            Flag::Seen => "\\Seen",
            Flag::Flagged => "\\Flagged",
        }
    }
}

pub enum Command {
    SetAccount(Account),
    /// The folders again, and the open folder's messages.
    Refresh,
    Open(String),
    Read {
        folder: String,
        uid: u32,
    },
    SetFlag {
        folder: String,
        uid: u32,
        flag: Flag,
        on: bool,
    },
    Move {
        folder: String,
        uid: u32,
        to: String,
    },
}

pub enum Update {
    Busy(bool),
    Folders(Vec<Folder>),
    Messages { folder: String, messages: Vec<Summary> },
    Body { folder: String, uid: u32, body: Box<Body> },
    Sent,
    Error(String),
}

pub struct Sync {
    tx: Sender<Command>,
    rx: Receiver<Update>,
    updates: Sender<Update>,
    ctx: egui::Context,
    pub busy: bool,
}

impl Sync {
    pub fn start(ctx: &egui::Context, account: Account) -> Self {
        let (tx, commands) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        let (worker_updates, worker_ctx) = (updates.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("imap".into())
            .spawn(move || worker(commands, worker_updates, worker_ctx, account))
            .expect("start the IMAP thread");
        Self { tx, rx, updates, ctx: ctx.clone(), busy: false }
    }

    /// A stand-in that never connects, for screenshots and tests.
    pub fn offline(ctx: &egui::Context) -> Self {
        let (tx, _) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        Self { tx, rx, updates, ctx: ctx.clone(), busy: false }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    /// Sends `message` over SMTP. Bridge files it in Sent itself.
    pub fn send_mail(&self, account: Account, message: lettre::Message) {
        let (updates, ctx) = (self.updates.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let update = match smtp(&account).and_then(|t| t.send(&message).map_err(|e| e.to_string())) {
                Ok(_) => Update::Sent,
                Err(e) => Update::Error(format!("Not sent: {e}")),
            };
            let _ = updates.send(update);
            ctx.request_repaint();
        });
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

fn connect(account: &Account) -> Result<Session, String> {
    let mode = match account.security {
        Security::StartTls => ConnectionMode::StartTls,
        Security::Ssl => ConnectionMode::Tls,
    };
    let client = ClientBuilder::new(account.host.as_str(), account.imap_port)
        .mode(mode)
        .tls_kind(TlsKind::Native)
        .danger_skip_tls_verify(account.is_local())
        .connect()
        .map_err(|e| format!("Can't reach Bridge at {}:{}: {e}", account.host, account.imap_port))?;
    client.login(&account.username, &account.password).map_err(|(e, _)| format!("Bridge refused the login: {e}"))
}

fn smtp(account: &Account) -> Result<SmtpTransport, String> {
    let params = TlsParameters::builder(account.host.clone())
        .dangerous_accept_invalid_certs(account.is_local())
        .build()
        .map_err(|e| e.to_string())?;
    let tls = match account.security {
        Security::StartTls => Tls::Required(params),
        Security::Ssl => Tls::Wrapper(params),
    };
    Ok(SmtpTransport::builder_dangerous(&account.host)
        .port(account.smtp_port)
        .tls(tls)
        .credentials(Credentials::new(account.username.clone(), account.password.clone()))
        .build())
}

fn worker(commands: Receiver<Command>, updates: Sender<Update>, ctx: egui::Context, mut account: Account) {
    let send = |u: Update| {
        let _ = updates.send(u);
        ctx.request_repaint();
    };
    let mut session: Option<Session> = None;
    let mut open: Option<String> = None;
    while let Ok(first) = commands.recv() {
        let mut queue = vec![first];
        queue.extend(commands.try_iter());
        send(Update::Busy(true));
        for command in queue {
            if let Command::SetAccount(a) = command {
                account = a;
                if let Some(mut s) = session.take() {
                    let _ = s.logout();
                }
                session = None;
                open = None;
                continue;
            }
            if !account.is_set() {
                send(Update::Error("Set up Bridge in File → Account…".into()));
                continue;
            }
            // One retry on a fresh connection: Bridge drops idle sessions.
            let mut result = Err(String::new());
            for attempt in 0..2 {
                if session.is_none() {
                    match connect(&account) {
                        Ok(s) => session = Some(s),
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    }
                }
                let s = session.as_mut().expect("connected");
                result = run(s, &command, &mut open, &send);
                match &result {
                    Err(e) if attempt == 0 && is_connection_error(e) => session = None,
                    _ => break,
                }
            }
            if let Err(e) = result {
                send(Update::Error(e));
            }
        }
        send(Update::Busy(false));
    }
}

fn is_connection_error(e: &str) -> bool {
    let e = e.to_ascii_lowercase();
    ["connection", "broken pipe", "bye", "eof", "reset"].iter().any(|w| e.contains(w))
}

fn run(s: &mut Session, command: &Command, open: &mut Option<String>, send: &impl Fn(Update)) -> Result<(), String> {
    let e = |e: imap::Error| e.to_string();
    match command {
        Command::SetAccount(_) => {}
        Command::Refresh => {
            send(Update::Folders(folders(s)?));
            if let Some(folder) = open.clone() {
                send(Update::Messages { messages: messages(s, &folder)?, folder });
            }
        }
        Command::Open(folder) => {
            *open = Some(folder.clone());
            send(Update::Messages { messages: messages(s, folder)?, folder: folder.clone() });
        }
        Command::Read { folder, uid } => {
            s.select(folder).map_err(e)?;
            let fetches = s.uid_fetch(uid.to_string(), "(UID BODY.PEEK[])").map_err(e)?;
            let raw = fetches.iter().find_map(|f| f.body()).ok_or("The message has gone")?;
            let body = Body::parse(raw).ok_or("Can't read this message")?;
            send(Update::Body { folder: folder.clone(), uid: *uid, body: Box::new(body) });
            s.uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)").map_err(e)?;
        }
        Command::SetFlag { folder, uid, flag, on } => {
            s.select(folder).map_err(e)?;
            let sign = if *on { '+' } else { '-' };
            s.uid_store(uid.to_string(), format!("{sign}FLAGS.SILENT ({})", flag.imap())).map_err(e)?;
            send(Update::Folders(folders(s)?));
        }
        Command::Move { folder, uid, to } => {
            s.select(folder).map_err(e)?;
            s.uid_mv(uid.to_string(), to).map_err(e)?;
            send(Update::Folders(folders(s)?));
            send(Update::Messages { messages: messages(s, folder)?, folder: folder.clone() });
        }
    }
    Ok(())
}

fn folders(s: &mut Session) -> Result<Vec<Folder>, String> {
    let names = s.list(None, Some("*")).map_err(|e| e.to_string())?;
    let mut found: Vec<(String, Option<String>)> = names
        .iter()
        // imap doesn't re-export imap-proto's NameAttribute, so match it by name.
        .filter(|n| !n.attributes().iter().any(|a| format!("{a:?}") == "NoSelect"))
        .map(|n| (n.name().to_string(), n.delimiter().map(str::to_string)))
        .collect();
    found.sort_by_key(|(name, _)| (rank(name), name.to_lowercase()));
    let mut folders = Vec::new();
    for (name, delimiter) in found {
        let unseen = s.status(&name, "(UNSEEN)").ok().and_then(|m| m.unseen).unwrap_or(0);
        folders.push(folder(name, delimiter.as_deref(), unseen));
    }
    Ok(folders)
}

fn folder(name: String, delimiter: Option<&str>, unseen: u32) -> Folder {
    let parts: Vec<&str> = match delimiter {
        Some(d) if !d.is_empty() => name.split(d).collect(),
        _ => vec![name.as_str()],
    };
    // Bridge puts folders under "Folders/" and labels under "Labels/": those
    // two stay as headings, and what's under them sits one level in.
    let label =
        if name.eq_ignore_ascii_case("INBOX") { "Inbox".into() } else { parts.last().unwrap_or(&"").to_string() };
    Folder { depth: parts.len() - 1, label, name, unseen }
}

/// Bridge's standard folders first, in the order Proton's web app shows them.
fn rank(name: &str) -> usize {
    const ORDER: &[&str] =
        &["INBOX", "Drafts", "Sent", "Starred", "Archive", "Spam", "Trash", "All Mail", "Folders", "Labels"];
    let top = name.split('/').next().unwrap_or(name);
    ORDER.iter().position(|o| o.eq_ignore_ascii_case(top)).unwrap_or(ORDER.len())
}

/// The newest `PAGE` messages in `folder`, newest first.
fn messages(s: &mut Session, folder: &str) -> Result<Vec<Summary>, String> {
    let mailbox = s.select(folder).map_err(|e| e.to_string())?;
    if mailbox.exists == 0 {
        return Ok(Vec::new());
    }
    let start = mailbox.exists.saturating_sub(PAGE - 1).max(1);
    let fetches =
        s.fetch(format!("{start}:*"), "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])").map_err(|e| e.to_string())?;
    let mut list: Vec<Summary> = fetches
        .iter()
        .filter_map(|f| {
            let flags = f.flags();
            let seen = flags.contains(&imap::types::Flag::Seen);
            let flagged = flags.contains(&imap::types::Flag::Flagged);
            let internal = f.internal_date().map(|d| d.timestamp()).unwrap_or_default();
            Some(Summary::parse(f.uid?, f.header().unwrap_or_default(), seen, flagged, internal))
        })
        .collect();
    list.sort_by(|a, b| b.date.cmp(&a.date).then(b.uid.cmp(&a.uid)));
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_named_and_nested() {
        let f = folder("Folders/Receipts".into(), Some("/"), 2);
        assert_eq!((f.label.as_str(), f.depth, f.unseen), ("Receipts", 1, 2));
        assert_eq!(folder("INBOX".into(), Some("/"), 0).label, "Inbox");
        assert_eq!(folder("Sent".into(), None, 0).depth, 0);
    }

    #[test]
    fn standard_folders_come_first() {
        let mut names = vec!["Labels/Work", "Trash", "Archive", "Folders/Bills", "INBOX", "Other", "Sent"];
        names.sort_by_key(|n| (rank(n), n.to_lowercase()));
        assert_eq!(names, vec!["INBOX", "Sent", "Archive", "Trash", "Folders/Bills", "Labels/Work", "Other"]);
    }

    /// Against the Bridge running on this machine, with the account in
    /// `MAIL_LIVE_ACCOUNT` (a path to an account.toml): `make live-test`.
    #[test]
    #[ignore]
    fn live_bridge_lists_the_inbox() {
        let path = std::env::var("MAIL_LIVE_ACCOUNT").expect("MAIL_LIVE_ACCOUNT");
        let account: Account = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut s = connect(&account).unwrap();
        let folders = folders(&mut s).unwrap();
        assert_eq!(folders[0].name, "INBOX");
        let list = messages(&mut s, "INBOX").unwrap();
        assert!(list.len() <= PAGE as usize);
        assert!(list.windows(2).all(|w| w[0].date >= w[1].date));
    }
}
