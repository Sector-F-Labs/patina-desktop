//! The bus, watched: a connection that asks the daemon for a copy of every message
//! (`org.freedesktop.DBus.Monitoring.BecomeMonitor`), the way `dbus-monitor` does. A
//! monitor can't send or reply, and the bus never waits for it. Each message becomes a
//! [`Record`], and a call's reply is matched to it by serial, with the time between.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use zbus::message::Type;
use zbus::{Connection, Message, MessageStream};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Call,
    Reply,
    Error,
    Signal,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Call => "call",
            Kind::Reply => "reply",
            Kind::Error => "error",
            Kind::Signal => "signal",
        }
    }
}

/// One message as the table shows it.
#[derive(Clone, Debug)]
pub struct Record {
    /// Counts up from 1 in the order the monitor saw them.
    pub seq: u64,
    pub at: SystemTime,
    pub kind: Kind,
    pub serial: u32,
    pub reply_serial: Option<u32>,
    pub sender: String,
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub member: String,
    pub signature: String,
    /// The arguments, written out.
    pub body: String,
    /// The arguments themselves, to send again or edit.
    pub args: Vec<zbus::zvariant::Value<'static>>,
    /// For a reply or an error: the call it answers, and how long it took.
    pub call_seq: Option<u64>,
    pub latency: Option<Duration>,
}

impl Record {
    /// All the text a filter looks in.
    pub fn haystack(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.kind.label(),
            self.sender,
            self.destination,
            self.path,
            self.interface,
            self.member,
            self.body
        )
    }
}

/// Turns messages into records, keeping the calls still waiting for an answer.
#[derive(Default)]
pub struct Recorder {
    seq: u64,
    pending: HashMap<(String, u32), (u64, Instant)>,
}

impl Recorder {
    pub fn record(&mut self, msg: &Message) -> Record {
        self.record_at(msg, Instant::now())
    }

    fn record_at(&mut self, msg: &Message, now: Instant) -> Record {
        self.seq += 1;
        let header = msg.header();
        let kind = match header.message_type() {
            Type::MethodCall => Kind::Call,
            Type::MethodReturn => Kind::Reply,
            Type::Error => Kind::Error,
            Type::Signal => Kind::Signal,
        };
        let text = |s: Option<String>| s.unwrap_or_default();
        let serial = header.primary().serial_num().get();
        let sender = text(header.sender().map(|s| s.to_string()));
        let destination = text(header.destination().map(|s| s.to_string()));
        let reply_serial = header.reply_serial().map(|s| s.get());
        let signature = signature_of(header.signature());
        let mut record = Record {
            seq: self.seq,
            at: SystemTime::now(),
            kind,
            serial,
            reply_serial,
            sender: sender.clone(),
            destination: destination.clone(),
            path: text(header.path().map(|s| s.to_string())),
            interface: text(header.interface().map(|s| s.to_string())),
            member: text(header.member().map(|s| s.to_string())),
            signature,
            body: arguments(msg),
            args: values(msg),
            call_seq: None,
            latency: None,
        };
        match kind {
            Kind::Call => {
                self.pending.insert((sender, serial), (self.seq, now));
            }
            Kind::Reply | Kind::Error => {
                if let Some(call) =
                    reply_serial.and_then(|s| self.pending.remove(&(destination, s)))
                {
                    record.call_seq = Some(call.0);
                    record.latency = Some(now.saturating_duration_since(call.1));
                }
            }
            Kind::Signal => {}
        }
        record
    }
}

/// The wire signature, as dbus-monitor writes it. zvariant reads a body's signature as one
/// struct of its arguments and prints it in parentheses, so one pair goes. That can't tell
/// two arguments `su` from a lone struct `(su)`: zvariant parses both alike.
fn signature_of(signature: &zbus::zvariant::Signature) -> String {
    let text = signature.to_string();
    match signature {
        zbus::zvariant::Signature::Structure(_) => text
            .strip_prefix('(')
            .and_then(|t| t.strip_suffix(')'))
            .unwrap_or(&text)
            .to_string(),
        _ => text,
    }
}

/// The message's arguments, as values that outlive it.
fn values(msg: &Message) -> Vec<zbus::zvariant::Value<'static>> {
    if msg.header().signature().to_string().is_empty() {
        return Vec::new();
    }
    let body = msg.body();
    let Ok(args) = body.deserialize::<zbus::zvariant::Structure>() else {
        return Vec::new();
    };
    args.fields()
        .iter()
        .filter_map(|v| v.try_to_owned().ok().map(zbus::zvariant::Value::from))
        .collect()
}

/// The message's arguments in GVariant's text form, or why they can't be read.
fn arguments(msg: &Message) -> String {
    let body = msg.body();
    if msg.header().signature().to_string().is_empty() {
        return String::new();
    }
    match body.deserialize::<zbus::zvariant::Structure>() {
        // The arguments are written as a struct; they aren't one, so the parentheses go.
        Ok(args) => {
            let text = format!("{args}");
            let inner = text
                .strip_prefix('(')
                .and_then(|t| t.strip_suffix(')'))
                .unwrap_or(&text);
            inner.strip_suffix(',').unwrap_or(inner).to_string()
        }
        Err(e) => format!("(unreadable: {e})"),
    }
}

/// What a monitor sends the window.
pub enum Event {
    Message(Box<Record>),
    /// What came of a message Dbug sent: the reply's arguments, or what went wrong.
    Sent {
        what: String,
        outcome: Result<String, String>,
    },
    /// The connection to the bus ended, or never opened.
    Closed(String),
}

/// Watches the session bus on a thread of its own, sending each message to `tx` and calling
/// `wake` after it so the window draws it. `rules` are match rules, empty for everything.
pub fn watch(rules: Vec<String>, tx: mpsc::Sender<Event>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let end = zbus::block_on(async {
            let connection = Connection::session().await.map_err(|e| e.to_string())?;
            become_monitor(&connection, &rules)
                .await
                .map_err(|e| e.to_string())?;
            let mut recorder = Recorder::default();
            let mut stream = MessageStream::from(connection);
            use futures_util::StreamExt;
            while let Some(msg) = stream.next().await {
                let msg = msg.map_err(|e| e.to_string())?;
                if tx
                    .send(Event::Message(Box::new(recorder.record(&msg))))
                    .is_err()
                {
                    return Ok(());
                }
                wake();
            }
            Ok::<(), String>(())
        });
        let _ = tx.send(Event::Closed(match end {
            Ok(()) => "the bus closed the connection".into(),
            Err(e) => e,
        }));
        wake();
    });
}

pub(crate) fn arguments_text(msg: &Message) -> String {
    arguments(msg)
}

async fn become_monitor(connection: &Connection, rules: &[String]) -> zbus::Result<()> {
    connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus.Monitoring"),
            "BecomeMonitor",
            &(rules, 0u32),
        )
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> Message {
        Message::method_call("/org/example/Thing", "Ping")
            .unwrap()
            .sender(":1.7")
            .unwrap()
            .destination("org.example.Thing")
            .unwrap()
            .interface("org.example.Iface")
            .unwrap()
            .build(&"hello")
            .unwrap()
    }

    #[test]
    fn a_call_is_recorded_with_its_arguments() {
        let r = Recorder::default().record(&call());
        assert_eq!(r.kind, Kind::Call);
        assert_eq!(r.sender, ":1.7");
        assert_eq!(r.destination, "org.example.Thing");
        assert_eq!(r.path, "/org/example/Thing");
        assert_eq!(r.interface, "org.example.Iface");
        assert_eq!(r.member, "Ping");
        assert_eq!(r.signature, "s");
        assert_eq!(r.body, "\"hello\"");
    }

    #[test]
    fn a_reply_finds_its_call_and_the_time_between() {
        let mut recorder = Recorder::default();
        let call = call();
        let t0 = Instant::now();
        let first = recorder.record_at(&call, t0);
        let reply = Message::method_return(&call.header())
            .unwrap()
            .sender(":1.9")
            .unwrap()
            .destination(":1.7")
            .unwrap()
            .build(&())
            .unwrap();
        let second = recorder.record_at(&reply, t0 + Duration::from_millis(12));
        assert_eq!(second.kind, Kind::Reply);
        assert_eq!(second.call_seq, Some(first.seq));
        assert_eq!(second.latency, Some(Duration::from_millis(12)));
        assert!(second.body.is_empty());
    }

    /// Needs a bus: `dbus-run-session -- cargo test -p patina-dbug -- --ignored`.
    #[test]
    #[ignore]
    fn the_monitor_sees_a_signal_sent_by_another_connection() {
        let (tx, rx) = mpsc::channel();
        watch(Vec::new(), tx, || {});
        std::thread::sleep(Duration::from_millis(500));
        // dbus-send puts the arguments on the wire as they are, not wrapped in a struct.
        let sent = std::process::Command::new("dbus-send")
            .args([
                "--session",
                "--type=signal",
                "/org/example/Sig",
                "org.example.Sig.Pinged",
                "string:x",
                "uint32:3",
            ])
            .status()
            .unwrap();
        assert!(sent.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Event::Message(r)) = rx.recv_timeout(Duration::from_millis(200))
                && r.member == "Pinged"
            {
                assert_eq!(r.kind, Kind::Signal);
                assert_eq!(r.interface, "org.example.Sig");
                assert_eq!(r.signature, "su");
                assert_eq!(r.body, "\"x\", uint32 3");
                return;
            }
        }
        panic!("the monitor never saw the signal");
    }
}
