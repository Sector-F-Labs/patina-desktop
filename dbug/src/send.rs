//! Sending: a second, ordinary connection to the bus. The monitor can't send (the bus
//! makes a monitor listen-only), so a call or signal goes out from here, and the monitor
//! sees it pass like any other message. Each message goes from a thread of its own, so a
//! call that's slow to answer holds up nothing.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use zbus::Connection;
use zbus::zvariant::{Structure, StructureBuilder, Value};

use crate::monitor::{self, Event};

/// How long a call waits for its answer.
const WAIT: Duration = Duration::from_secs(25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Call,
    Signal,
}

#[derive(Clone, Debug)]
pub struct Outgoing {
    pub kind: Kind,
    /// A call's recipient; a signal's, or empty for everyone listening.
    pub destination: String,
    pub path: String,
    pub interface: String,
    pub member: String,
    pub args: Vec<Value<'static>>,
}

impl Outgoing {
    fn what(&self) -> String {
        let to = if self.destination.is_empty() {
            String::new()
        } else {
            format!(" → {}", self.destination)
        };
        format!(
            "{} {}.{}{to}",
            if self.kind == Kind::Call {
                "call"
            } else {
                "signal"
            },
            self.interface,
            self.member
        )
    }
}

#[derive(Clone)]
pub struct Sender {
    connection: Arc<Mutex<Option<Connection>>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Sender {
    pub fn new(events: mpsc::Sender<Event>, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            connection: Arc::default(),
            events,
            wake: Arc::new(wake),
        }
    }

    /// Sends it; the outcome comes back as an [`Event::Sent`].
    pub fn send(&self, out: Outgoing) {
        let me = self.clone();
        std::thread::spawn(move || {
            let what = out.what();
            let outcome = zbus::block_on(me.run(out));
            let _ = me.events.send(Event::Sent { what, outcome });
            (me.wake)();
        });
    }

    async fn connection(&self) -> Result<Connection, String> {
        if let Some(c) = self.connection.lock().unwrap().clone() {
            return Ok(c);
        }
        let c = Connection::session().await.map_err(|e| e.to_string())?;
        *self.connection.lock().unwrap() = Some(c.clone());
        Ok(c)
    }

    async fn run(&self, out: Outgoing) -> Result<String, String> {
        let connection = self.connection().await?;
        if out.args.is_empty() {
            deliver(&connection, &out, &()).await
        } else {
            deliver(&connection, &out, &body(&out.args)?).await
        }
    }
}

async fn deliver<B>(connection: &Connection, out: &Outgoing, body: &B) -> Result<String, String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let destination = (!out.destination.is_empty()).then_some(out.destination.as_str());
    match out.kind {
        Kind::Signal => {
            connection
                .emit_signal(
                    destination,
                    out.path.as_str(),
                    out.interface.as_str(),
                    out.member.as_str(),
                    body,
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok("Sent. A signal has no reply.".into())
        }
        Kind::Call => {
            let Some(destination) = destination else {
                return Err("A call needs a destination: the name of the program to ask.".into());
            };
            let call = connection.call_method(
                Some(destination),
                out.path.as_str(),
                Some(out.interface.as_str()),
                out.member.as_str(),
                body,
            );
            let timeout = async {
                async_io::Timer::after(WAIT).await;
                Err(zbus::Error::InputOutput(Arc::new(std::io::Error::other(
                    format!("no answer in {} seconds", WAIT.as_secs()),
                ))))
            };
            let reply = futures_util::future::select(Box::pin(call), Box::pin(timeout))
                .await
                .factor_first()
                .0;
            match reply {
                Ok(msg) => {
                    let text = monitor::arguments_text(&msg);
                    Ok(if text.is_empty() {
                        "Answered with nothing.".into()
                    } else {
                        text
                    })
                }
                Err(zbus::Error::MethodError(name, description, _)) => Err(format!(
                    "{name}{}",
                    description.map(|d| format!(": {d}")).unwrap_or_default()
                )),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

/// The arguments as a message's body: the values side by side, which a struct's fields are
/// when it's sent as the body.
fn body(args: &[Value<'static>]) -> Result<Structure<'static>, String> {
    let mut b = StructureBuilder::new();
    for a in args {
        b = b.append_field(a.try_clone().map_err(|e| e.to_string())?);
    }
    b.build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn outcome(out: Outgoing) -> Result<String, String> {
        let (tx, rx) = mpsc::channel();
        Sender::new(tx, || {}).send(out);
        match rx
            .recv_timeout(Duration::from_secs(10))
            .expect("no outcome")
        {
            Event::Sent { outcome, .. } => outcome,
            _ => panic!("not an outcome"),
        }
    }

    fn call(member: &str, args: Vec<Value<'static>>) -> Outgoing {
        Outgoing {
            kind: Kind::Call,
            destination: "org.freedesktop.DBus".into(),
            path: "/org/freedesktop/DBus".into(),
            interface: "org.freedesktop.DBus".into(),
            member: member.into(),
            args,
        }
    }

    /// Needs a bus: `dbus-run-session -- cargo test -p patina-dbug -- --ignored`.
    #[test]
    #[ignore]
    fn a_call_gets_its_answer() {
        let r = outcome(call(
            "NameHasOwner",
            vec![Value::from("org.freedesktop.DBus")],
        ))
        .unwrap();
        assert_eq!(r, "true");
    }

    #[test]
    #[ignore]
    fn a_call_the_bus_refuses_comes_back_as_its_error() {
        let e = outcome(call("NameHasOwner", vec![Value::U32(3)])).unwrap_err();
        assert!(e.contains("org.freedesktop.DBus.Error"), "{e}");
    }

    #[test]
    #[ignore]
    fn a_call_without_a_destination_is_refused_here() {
        let mut out = call("ListNames", vec![]);
        out.destination.clear();
        assert!(outcome(out).unwrap_err().contains("destination"));
    }

    /// What Dbug sends, the monitor sees, with the arguments side by side as a program's own.
    #[test]
    #[ignore]
    fn what_is_sent_passes_the_monitor() {
        let (tx, rx) = mpsc::channel();
        monitor::watch(Vec::new(), tx, || {});
        std::thread::sleep(Duration::from_millis(500));
        let out = Outgoing {
            kind: Kind::Signal,
            destination: String::new(),
            path: "/org/example/Sig".into(),
            interface: "org.example.Sig".into(),
            member: "Sent".into(),
            args: vec![Value::from("x"), Value::U32(3)],
        };
        let (tx2, _keep) = mpsc::channel();
        Sender::new(tx2, || {}).send(out);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Event::Message(r)) = rx.recv_timeout(Duration::from_millis(200))
                && r.member == "Sent"
            {
                assert_eq!(r.kind, monitor::Kind::Signal);
                assert_eq!(r.signature, "su");
                assert_eq!(r.body, "\"x\", uint32 3");
                return;
            }
        }
        panic!("the monitor never saw it");
    }
}
