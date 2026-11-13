//! The prompter: owns the keyring prompter's names on the session bus and
//! answers gcr's internal prompter protocol (`org.gnome.keyring.internal.
//! Prompter`), showing each prompt in a dialog (`dialog.rs`, run as a child).
//!
//! A client (gnome-keyring-daemon, through libgcr) calls `BeginPrompting`
//! with a callback object; we call its `PromptReady` when it may ask. It then
//! calls `PerformPrompt` with what to show, and we call `PromptReady` again
//! with `yes` or `no` and, for a password, the password under the secret
//! exchange (`exchange.rs`). `StopPrompting` ends it with `PromptDone`.

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::dialog::Request;
use crate::exchange::Exchange;

const NAMES: [&str; 2] = [
    "org.gnome.keyring.SystemPrompter",
    "org.gnome.keyring.PrivatePrompter",
];
const PATH: &str = "/org/gnome/keyring/Prompter";
const CALLBACK: &str = "org.gnome.keyring.internal.Prompter.Callback";

/// One dialog at a time, as gcr's system prompter does.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One client's conversation: its keys, and the dialog on screen for it.
struct Session {
    exchange: Mutex<Exchange>,
    dialog: AtomicU32,
    cancelled: AtomicBool,
}

impl Session {
    fn new() -> Session {
        Session {
            exchange: Mutex::new(Exchange::new()),
            dialog: AtomicU32::new(0),
            cancelled: AtomicBool::new(false),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let pid = self.dialog.load(Ordering::SeqCst);
        if pid != 0 {
            // SAFETY: a signal to the dialog's process, which `ask` reaps
            // only after it ends.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// A client is its bus name and its callback's path.
type Key = (String, String);
type Sessions = Arc<Mutex<HashMap<Key, Arc<Session>>>>;

/// What the dialog answered.
struct Answer {
    password: Option<String>,
    choice: bool,
    /// For a new password: how good the dialog thinks it is.
    strength: Option<i32>,
}

/// This program's path, for running itself as the dialog. After a reinstall
/// replaces the file under a running prompter Linux reports the path with a
/// " (deleted)" suffix; the new binary at the plain path is what to run.
fn own_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(strip_deleted(exe))
}

fn strip_deleted(exe: PathBuf) -> PathBuf {
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(plain) => plain.into(),
        None => exe,
    }
}

/// Shows the dialog and waits for its answer; `None` when it was closed.
fn ask(session: &Session, request: &Request) -> Result<Option<Answer>, String> {
    let _one = lock(&ONE_AT_A_TIME);
    if session.is_cancelled() {
        return Ok(None);
    }
    // PATINA_KEYRING_DIALOG replaces the dialog with another program, which
    // gets the same options: for tests with no screen (tests/daemon-e2e.py).
    let mut command = match std::env::var_os("PATINA_KEYRING_DIALOG") {
        Some(program) => Command::new(program),
        None => {
            let mut command = Command::new(own_path()?);
            command.arg("--ask");
            command
        }
    };
    let mut child = command
        .args(request.to_args())
        // Held open: the dialog closes itself if this side goes away.
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("the dialog didn't start: {e}"))?;
    session.dialog.store(child.id(), Ordering::SeqCst);
    if session.is_cancelled() {
        // SAFETY: as in `cancel`.
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    }
    let stdin = child.stdin.take();
    let mut out = String::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = BufReader::new(stdout.take(65536)).read_to_string(&mut out);
    }
    let _ = child.wait();
    drop(stdin);
    session.dialog.store(0, Ordering::SeqCst);
    let answer = parse_answer(&out);
    wipe(&mut out);
    Ok(answer)
}

fn parse_answer(out: &str) -> Option<Answer> {
    let mut lines = out.lines();
    if lines.next()? != "yes" {
        return None;
    }
    let mut answer = Answer {
        password: None,
        choice: false,
        strength: None,
    };
    for line in lines {
        if let Some(c) = line.strip_prefix("choice ") {
            answer.choice = c == "1";
        } else if let Some(s) = line.strip_prefix("strength ") {
            answer.strength = s.parse().ok();
        } else if let Some(p) = line.strip_prefix("password ") {
            answer.password = Some(p.to_string());
        }
    }
    Some(answer)
}

fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// A property as `PromptReady` carries it: gcr's prompter wraps each value in
/// a variant of its own inside the `a{sv}`'s, and the client unwraps it, so a
/// bare value is ignored. (The client's own properties aren't wrapped.)
fn nested(value: Value<'_>) -> Value<'_> {
    Value::Value(Box::new(value))
}

/// Calls the client's `PromptReady`.
fn ready(
    bus: &zbus::blocking::Connection,
    key: &Key,
    reply: &str,
    properties: HashMap<&str, Value<'_>>,
    exchange: &str,
) {
    if let Err(e) = bus.call_method(
        Some(key.0.as_str()),
        key.1.as_str(),
        Some(CALLBACK),
        "PromptReady",
        &(reply, properties, exchange),
    ) {
        eprintln!("patina-keyring: PromptReady on {}: {e}", key.0);
    }
}

struct Prompter {
    bus: zbus::blocking::Connection,
    sessions: Sessions,
}

fn sender(header: &zbus::message::Header<'_>) -> zbus::fdo::Result<String> {
    header
        .sender()
        .map(|s| s.to_string())
        .ok_or_else(|| zbus::fdo::Error::Failed("no sender".into()))
}

#[zbus::interface(name = "org.gnome.keyring.internal.Prompter")]
impl Prompter {
    async fn begin_prompting(
        &self,
        callback: ObjectPath<'_>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let key = (sender(&header)?, callback.to_string());
        let session = Arc::new(Session::new());
        let begin = lock(&session.exchange).begin();
        lock(&self.sessions).insert(key.clone(), session);
        // Off the bus's thread; it has to answer this call first.
        let bus = self.bus.clone();
        std::thread::spawn(move || ready(&bus, &key, "", HashMap::new(), &begin));
        Ok(())
    }

    async fn perform_prompt(
        &self,
        callback: ObjectPath<'_>,
        kind: String,
        properties: HashMap<String, OwnedValue>,
        exchange: String,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let key = (sender(&header)?, callback.to_string());
        let session = lock(&self.sessions)
            .get(&key)
            .cloned()
            .ok_or_else(|| zbus::fdo::Error::Failed("not prompting".into()))?;
        lock(&session.exchange)
            .receive(&exchange)
            .map_err(zbus::fdo::Error::Failed)?;
        let request = Request::from_prompt(&kind, &properties);
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            let answer = match ask(&session, &request) {
                Ok(answer) => answer,
                Err(e) => {
                    eprintln!("patina-keyring: {e}");
                    None
                }
            };
            // Stopped meanwhile: `StopPrompting` ends it with PromptDone.
            if session.is_cancelled() {
                return;
            }
            let exchange = lock(&session.exchange);
            let (reply, props, out) = match answer {
                Some(mut a) => {
                    let mut props = HashMap::new();
                    if request.has_choice() {
                        props.insert("choice-chosen", nested(Value::from(a.choice)));
                    }
                    if let Some(strength) = a.strength {
                        props.insert("password-strength", nested(Value::from(strength)));
                    }
                    let out = match a.password.as_deref() {
                        Some(p) => exchange.send(Some(p.as_bytes())),
                        None => exchange.send(None),
                    };
                    if let Some(p) = a.password.as_mut() {
                        wipe(p);
                    }
                    ("yes", props, out)
                }
                None => ("no", HashMap::new(), exchange.send(None)),
            };
            match out {
                Ok(out) => ready(&bus, &key, reply, props, &out),
                Err(e) => eprintln!("patina-keyring: {e}"),
            }
        });
        Ok(())
    }

    async fn stop_prompting(
        &self,
        callback: ObjectPath<'_>,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let key = (sender(&header)?, callback.to_string());
        if let Some(session) = lock(&self.sessions).remove(&key) {
            session.cancel();
        }
        let bus = self.bus.clone();
        std::thread::spawn(move || {
            let _ = bus.call_method(
                Some(key.0.as_str()),
                key.1.as_str(),
                Some(CALLBACK),
                "PromptDone",
                &(),
            );
        });
        Ok(())
    }
}

pub fn run() -> Result<ExitCode, String> {
    let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let sessions = Sessions::default();
    bus.object_server()
        .at(
            PATH,
            Prompter {
                bus: bus.clone(),
                sessions: sessions.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
    for name in NAMES {
        bus.request_name(name)
            .map_err(|e| format!("couldn't take {name} (is another prompter running?): {e}"))?;
    }
    eprintln!("patina-keyring: the keyring prompter");

    // A client that goes away leaves nothing to ask for: close its dialog.
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).map_err(|e| e.to_string())?;
    let changes = dbus
        .receive_name_owner_changed()
        .map_err(|e| e.to_string())?;
    for change in changes {
        let Ok(args) = change.args() else { continue };
        if args.new_owner().is_some() {
            continue;
        }
        let gone = args.name().to_string();
        lock(&sessions).retain(|key, session| {
            let keep = key.0 != gone;
            if !keep {
                session.cancel();
            }
            keep
        });
    }
    Ok(ExitCode::SUCCESS)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replaced_binarys_path_loses_the_deleted_suffix() {
        let strip = |s: &str| strip_deleted(s.into());
        assert_eq!(
            strip("/usr/local/bin/patina-keyring (deleted)"),
            PathBuf::from("/usr/local/bin/patina-keyring")
        );
        assert_eq!(
            strip("/usr/local/bin/patina-keyring"),
            PathBuf::from("/usr/local/bin/patina-keyring")
        );
    }

    #[test]
    fn the_dialogs_answers_are_understood() {
        let a = parse_answer("yes\nchoice 1\nstrength 3\npassword hunter2 x\n").unwrap();
        assert_eq!(a.password.as_deref(), Some("hunter2 x"));
        assert_eq!(a.strength, Some(3));
        assert!(a.choice);
        let a = parse_answer("yes\nchoice 0\n").unwrap();
        assert_eq!(a.password, None);
        assert!(!a.choice);
        assert!(parse_answer("no\n").is_none());
        assert!(parse_answer("").is_none());
    }
}
