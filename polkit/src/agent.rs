//! The agent: registers with polkit's Authority on the system bus for this
//! login session, and answers `BeginAuthentication` by asking for the
//! password in a dialog (`dialog.rs`, run as a child) and checking it with
//! polkit's own helper, as polkit's other agents do.
//!
//! The helper does the PAM conversation as root (a socket-activated service
//! on current polkit, a setuid program on older ones) and, once the password
//! is right, tells the Authority itself; this side only carries its prompts
//! to the dialog and the answers back.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use zbus::zvariant::{OwnedValue, Value};

const AUTHORITY: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const PATH: &str = "/com/sectorflabs/Patina/PolicyKit1/AuthenticationAgent";
/// Current polkit's helper, as a service systemd starts for each connection.
const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
/// Older polkit's setuid helper, where it's installed.
const HELPERS: [&str; 3] = [
    "/usr/lib/polkit-1/polkit-agent-helper-1",
    "/usr/libexec/polkit-agent-helper-1",
    "/usr/local/lib/polkit-1/polkit-agent-helper-1",
];
/// Passwords tried before it gives up, when the helper keeps saying no and
/// the dialog isn't closed. Counts only sessions that got as far as asking.
const TRIES: usize = 5;

/// What the helper says, a line at a time.
#[derive(Debug, PartialEq)]
enum Line<'a> {
    /// It wants an answer; the prompt, e.g. "Password: ".
    Prompt(&'a str),
    /// A message for the user.
    Info(&'a str),
    Success,
    Failure,
    Other,
}

fn parse(line: &str) -> Line<'_> {
    let line = line.trim_end_matches(['\n', '\r']);
    for key in ["PAM_PROMPT_ECHO_OFF", "PAM_PROMPT_ECHO_ON"] {
        if let Some(rest) = line.strip_prefix(key) {
            return Line::Prompt(rest.trim());
        }
    }
    for key in ["PAM_ERROR_MSG", "PAM_TEXT_INFO"] {
        if let Some(rest) = line.strip_prefix(key) {
            return Line::Info(rest.trim());
        }
    }
    match line {
        "SUCCESS" => Line::Success,
        "FAILURE" => Line::Failure,
        _ => Line::Other,
    }
}

/// Why an authentication ended without a yes.
#[derive(Debug)]
enum Failure {
    /// The dialog was closed, or polkit cancelled the request.
    Cancelled,
    Other(String),
}

/// Whether polkit has cancelled a request, and the dialog to close if so.
#[derive(Default)]
struct Cancel {
    cancelled: AtomicBool,
    dialog: AtomicU32,
}

impl Cancel {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let pid = self.dialog.load(Ordering::SeqCst);
        if pid != 0 {
            // SAFETY: a signal to the dialog's process, which the worker
            // reaps only after the request ends.
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// One conversation with the helper: it's told who and which cookie, then
/// sends prompts and takes answers.
struct Helper {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
    child: Option<Child>,
}

impl Helper {
    fn open(user: &str, cookie: &str) -> Result<Helper, String> {
        if let Ok(stream) = UnixStream::connect(HELPER_SOCKET) {
            let reader = stream.try_clone().map_err(|e| e.to_string())?;
            let mut helper = Helper {
                reader: Box::new(BufReader::new(reader)),
                writer: Box::new(stream),
                child: None,
            };
            // The socket-activated helper isn't told who by its arguments.
            helper.say(user)?;
            helper.say(cookie)?;
            return Ok(helper);
        }
        let program = HELPERS
            .iter()
            .find(|p| std::path::Path::new(p).exists())
            .ok_or("polkit's agent helper isn't installed")?;
        let mut child = Command::new(program)
            .arg(user)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program}: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let mut helper = Helper {
            reader: Box::new(BufReader::new(stdout)),
            writer: Box::new(stdin),
            child: Some(child),
        };
        helper.say(cookie)?;
        Ok(helper)
    }

    fn say(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.writer, "{line}")
            .and_then(|()| self.writer.flush())
            .map_err(|e| e.to_string())
    }

    /// The next line, `None` when the helper is done.
    fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line),
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The dialog, a copy of this program run with `--ask`. Lines to it:
/// `prompt TEXT`, `info TEXT`, `wrong`, `ok`; from it: `password TEXT`.
struct Dialog {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// This program's path, for running itself as the dialog. After a reinstall
/// replaces the file under a running agent, Linux reports the path with a
/// " (deleted)" suffix; the new binary at the plain path is what to run.
fn own_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(strip_deleted(exe))
}

fn strip_deleted(exe: std::path::PathBuf) -> std::path::PathBuf {
    match exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(plain) => plain.into(),
        None => exe,
    }
}

impl Dialog {
    fn open(user: &str, message: &str) -> Result<Dialog, String> {
        let mut child = Command::new(own_path()?)
            .args(["--ask", "--user", user, "--message", message])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("the dialog didn't start: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        Ok(Dialog {
            child,
            stdin,
            stdout,
        })
    }

    fn tell(&mut self, word: &str, text: &str) {
        let text = text.replace(['\n', '\r'], " ");
        let _ = writeln!(self.stdin, "{word} {text}").and_then(|()| self.stdin.flush());
    }

    /// The password typed, or `None` when the dialog was closed.
    fn password(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line) {
            Ok(n) if n > 0 => line
                .trim_end_matches(['\n', '\r'])
                .strip_prefix("password ")
                .map(str::to_string),
            _ => None,
        }
    }
}

impl Drop for Dialog {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Asks for the password until the helper accepts one.
fn authenticate(user: &str, message: &str, cookie: &str, cancel: &Cancel) -> Result<(), Failure> {
    let mut dialog = Dialog::open(user, message).map_err(|e| {
        eprintln!("patina-polkit: {e}");
        Failure::Other(e)
    })?;
    cancel.dialog.store(dialog.child.id(), Ordering::SeqCst);
    // Cancelled while it was starting.
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    let mut asked = 0;
    let mut silent = 0;
    while asked < TRIES && silent < 3 {
        let mut helper = Helper::open(user, cookie).map_err(Failure::Other)?;
        let mut prompted = false;
        loop {
            let Some(line) = helper.line() else {
                // Gone without a verdict: refused.
                dialog.tell("wrong", "");
                break;
            };
            match parse(&line) {
                Line::Prompt(prompt) => {
                    prompted = true;
                    dialog.tell("prompt", prompt);
                    let Some(mut password) = dialog.password() else {
                        return Err(Failure::Cancelled);
                    };
                    let sent = helper.say(&password);
                    wipe(&mut password);
                    sent.map_err(Failure::Other)?;
                }
                Line::Info(text) => dialog.tell("info", text),
                Line::Success => {
                    dialog.tell("ok", "");
                    return Ok(());
                }
                Line::Failure => {
                    dialog.tell("wrong", "");
                    break;
                }
                Line::Other => {}
            }
        }
        if cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        if prompted {
            asked += 1;
        } else {
            silent += 1;
        }
    }
    Err(Failure::Other("too many wrong passwords".into()))
}

fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

/// The user to ask for: whoever is running this if polkit allows them (the
/// usual: administrators authenticate as themselves), else the first user it
/// lists (root, where no one here is an administrator).
fn pick_user(identities: &[(String, HashMap<String, OwnedValue>)], me: u32) -> Option<String> {
    let uids: Vec<u32> = identities
        .iter()
        .filter(|(kind, _)| kind == "unix-user")
        .filter_map(|(_, details)| details.get("uid")?.downcast_ref::<u32>().ok())
        .collect();
    let uid = if uids.contains(&me) {
        me
    } else {
        *uids.first()?
    };
    user_name(uid)
}

fn user_name(uid: u32) -> Option<String> {
    // SAFETY: getpwuid's result is read before any other call could replace it.
    unsafe {
        let entry = libc::getpwuid(uid);
        if entry.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr((*entry).pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

struct Agent {
    active: Arc<Mutex<HashMap<String, Arc<Cancel>>>>,
}

#[zbus::interface(name = "org.freedesktop.PolicyKit1.AuthenticationAgent")]
impl Agent {
    async fn begin_authentication(
        &self,
        action_id: String,
        message: String,
        _icon_name: String,
        _details: HashMap<String, String>,
        cookie: String,
        identities: Vec<(String, HashMap<String, OwnedValue>)>,
    ) -> zbus::fdo::Result<()> {
        // SAFETY: getuid never fails.
        let me = unsafe { libc::getuid() };
        let user = pick_user(&identities, me)
            .ok_or_else(|| zbus::fdo::Error::Failed("no one is allowed to authenticate".into()))?;
        let message = if message.is_empty() {
            action_id
        } else {
            message
        };
        let cancel = Arc::new(Cancel::default());
        lock(&self.active).insert(cookie.clone(), cancel.clone());
        // Off the bus's thread: it has to stay free for CancelAuthentication.
        let (done, result) = async_channel::bounded(1);
        let id = cookie.clone();
        let worker = cancel.clone();
        std::thread::spawn(move || {
            let _ = done.send_blocking(authenticate(&user, &message, &id, &worker));
        });
        let result = result.recv().await;
        lock(&self.active).remove(&cookie);
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(Failure::Cancelled)) => Err(zbus::fdo::Error::Failed("Cancelled".into())),
            Ok(Err(Failure::Other(e))) => Err(zbus::fdo::Error::Failed(e)),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn cancel_authentication(&self, cookie: String) {
        if let Some(cancel) = lock(&self.active).get(&cookie) {
            cancel.cancel();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// This login session's id: from the environment where PAM's logind module
/// set it, else logind's own idea of the caller's session.
fn session_id(bus: &zbus::blocking::Connection) -> Result<String, String> {
    if let Some(id) = std::env::var("XDG_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty())
    {
        return Ok(id);
    }
    let reply = bus
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.login1.Session", "Id"),
        )
        .map_err(|e| format!("which login session this is: {e}"))?;
    let body = reply.body();
    let value: Value = body.deserialize().map_err(|e| e.to_string())?;
    String::try_from(value).map_err(|e| e.to_string())
}

fn register(bus: &zbus::blocking::Connection, session: &str) -> Result<(), String> {
    let mut subject: HashMap<&str, Value> = HashMap::new();
    subject.insert("session-id", Value::from(session));
    let locale = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into());
    bus.call_method(
        Some(AUTHORITY),
        AUTHORITY_PATH,
        Some("org.freedesktop.PolicyKit1.Authority"),
        "RegisterAuthenticationAgent",
        &(("unix-session", subject), locale, PATH),
    )
    .map(drop)
    .map_err(|e| format!("couldn't register with polkit: {e}"))
}

pub fn run() -> Result<ExitCode, String> {
    let bus = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    bus.object_server()
        .at(
            PATH,
            Agent {
                active: Arc::default(),
            },
        )
        .map_err(|e| e.to_string())?;
    let session = session_id(&bus)?;
    register(&bus, &session)?;
    eprintln!("patina-polkit: the agent for session {session}");

    // polkitd starting again forgets its agents: register again.
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).map_err(|e| e.to_string())?;
    let changes = dbus
        .receive_name_owner_changed_with_args(&[(0, AUTHORITY)])
        .map_err(|e| e.to_string())?;
    for change in changes {
        let Ok(args) = change.args() else { continue };
        if args.new_owner().is_some()
            && let Err(e) = register(&bus, &session)
        {
            eprintln!("patina-polkit: {e}");
        }
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
            strip("/usr/local/bin/patina-polkit (deleted)"),
            std::path::PathBuf::from("/usr/local/bin/patina-polkit")
        );
        assert_eq!(
            strip("/usr/local/bin/patina-polkit"),
            std::path::PathBuf::from("/usr/local/bin/patina-polkit")
        );
    }

    #[test]
    fn the_helpers_lines_are_understood() {
        assert_eq!(
            parse("PAM_PROMPT_ECHO_OFF Password: \n"),
            Line::Prompt("Password:")
        );
        assert_eq!(parse("PAM_ERROR_MSG Try again\n"), Line::Info("Try again"));
        assert_eq!(parse("SUCCESS\n"), Line::Success);
        assert_eq!(parse("FAILURE\n"), Line::Failure);
        assert_eq!(parse("polkit-agent-helper-1: something\n"), Line::Other);
    }

    #[test]
    fn the_user_asked_for_is_the_one_running_this_if_allowed() {
        let uid = |n: u32| {
            (
                "unix-user".to_string(),
                HashMap::from([("uid".to_string(), OwnedValue::from(n))]),
            )
        };
        let root = user_name(0);
        // Root alone: root, whoever is running this.
        assert_eq!(pick_user(&[uid(0)], 4_000_000), root);
        // Both: the running user.
        assert_eq!(pick_user(&[uid(1), uid(0)], 0), root);
        assert_eq!(pick_user(&[], 0), None);
        let group = ("unix-group".to_string(), HashMap::new());
        assert_eq!(pick_user(&[group], 0), None);
    }
}
