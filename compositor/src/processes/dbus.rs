//! The desktop's processes on the session bus (ADR-0045), for Settings'
//! Processes page, and `Spawn` for the bar and the desktop's menu to start
//! their menus and Settings here, so they're listed too:
//! `com.sectorflabs.Patina.Processes` at `/com/sectorflabs/Patina/Processes`.
//! The list is global, so the interface reads and changes it from zbus's
//! threads, with no trip through the event loop. There are no signals;
//! readers poll. `RestartDesktop` is the exception: it goes to the loop,
//! which quits for patina-session to start it again (ADR-0046).

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::mpsc;
use std::time::Duration;

use smithay::reexports::calloop::{LoopHandle, channel};

use crate::state::State;

pub const NAME: &str = "com.sectorflabs.Patina.Processes";
pub const PATH: &str = "/com/sectorflabs/Patina/Processes";

/// A request to restart the desktop, and where to say how it went.
type Restart = mpsc::Sender<Result<(), String>>;

/// Claims the name and serves the interface while the connection lives.
/// What's spawned is a client of the compositor at `socket`; restarting
/// the desktop is asked of the loop at `handle`.
pub fn serve(
    socket: &OsStr,
    handle: &LoopHandle<'static, State>,
) -> Result<zbus::blocking::Connection, String> {
    let (restart, asked) = channel::channel::<Restart>();
    handle
        .insert_source(asked, |event, _, state| {
            if let channel::Event::Msg(reply) = event {
                let _ = reply.send(state.restart_desktop());
            }
        })
        .map_err(|e| e.to_string())?;
    serve_with(socket, restart)
}

fn serve_with(
    socket: &OsStr,
    restart: channel::Sender<Restart>,
) -> Result<zbus::blocking::Connection, String> {
    let interface = Interface {
        socket: socket.to_os_string(),
        restart,
    };
    zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, interface))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

struct Interface {
    socket: OsString,
    restart: channel::Sender<Restart>,
}

fn failed(e: String) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e)
}

#[zbus::interface(name = "com.sectorflabs.Patina.Processes")]
impl Interface {
    /// Each process: id, name, command line, PID (0 when not running),
    /// status, whether it's kept, restarts, and when it last started
    /// (seconds since the epoch).
    #[allow(clippy::type_complexity)]
    fn list(&self) -> Vec<(u32, String, String, u32, String, bool, u32, u64)> {
        super::list()
            .into_iter()
            .map(|e| {
                (
                    e.id, e.name, e.command, e.pid, e.status, e.kept, e.restarts, e.started,
                )
            })
            .collect()
    }

    /// Starts `command` through sh(1) as a client of the compositor, with
    /// `env` besides its own variables (not WAYLAND_DISPLAY, DISPLAY, PATH
    /// or XDG_*), and returns its id. It runs once.
    fn spawn(&self, command: String, env: HashMap<String, String>) -> zbus::fdo::Result<u32> {
        if command.trim().is_empty() {
            return Err(zbus::fdo::Error::InvalidArgs("no command".into()));
        }
        Ok(super::spawn_with(
            &self.socket,
            &command,
            false,
            env.into_iter().collect(),
        ))
    }

    fn restart(&self, id: u32) -> zbus::fdo::Result<()> {
        super::restart(id).map_err(failed)
    }

    fn stop(&self, id: u32) -> zbus::fdo::Result<()> {
        super::stop(id).map_err(failed)
    }

    /// Quits every window and starts the desktop again, under
    /// patina-session; refused without it, or while the screen is locked.
    fn restart_desktop(&self) -> zbus::fdo::Result<()> {
        let (reply, result) = mpsc::channel();
        self.restart
            .send(reply)
            .map_err(|e| failed(e.to_string()))?;
        result
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| failed(e.to_string()))?
            .map_err(failed)
    }

    /// The desktop can restart: patina-session started the compositor.
    #[zbus(property)]
    fn supervised(&self) -> bool {
        crate::session::supervised()
    }

    /// The process's lines from sequence number `from` on, and the number
    /// to ask from next.
    fn log(&self, id: u32, from: u64) -> zbus::fdo::Result<(u64, Vec<String>)> {
        super::log(id, from).ok_or_else(|| failed(format!("no process {id}")))
    }
}

/// On a bus of its own: `dbus-run-session -- cargo test -p
/// patina-compositor processes -- --ignored`.
#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use patina_system::processes::Processes;

    #[test]
    #[ignore = "needs a session bus of its own: run under dbus-run-session"]
    fn settings_lists_reads_restarts_and_stops() {
        let (restart, _) = smithay::reexports::calloop::channel::channel();
        let _served = super::serve_with("test".as_ref(), restart)
            .expect("the name is free on a bus of our own");
        super::super::spawn("test".as_ref(), "echo ready; sleep 30", true);
        let client = Processes::connect().unwrap();
        assert!(client.served().unwrap());
        let wait = |done: &dyn Fn(&patina_system::processes::Process) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let list = client.list().unwrap();
                let p = list
                    .iter()
                    .find(|p| p.command == "echo ready; sleep 30")
                    .unwrap();
                if done(p) {
                    return p.clone();
                }
                assert!(Instant::now() < deadline, "{p:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let p = wait(&|p| p.running());
        assert_eq!(p.name, "echo");
        assert!(p.kept);
        let deadline = Instant::now() + Duration::from_secs(5);
        while client.log(p.id, 0).unwrap().1.is_empty() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(client.log(p.id, 0).unwrap(), (1, vec!["ready".to_string()]));
        client.restart(p.id).unwrap();
        wait(&|p| p.running() && p.restarts == 1);
        client.stop(p.id).unwrap();
        wait(&|p| p.status == "stopped");
        assert!(client.log(9999, 0).is_err());

        let env = [("PATINA_SKIN", "Platinum"), ("PATH", "/nowhere")];
        let id = patina_system::processes::spawn("echo $PATINA_SKIN; command -v sh", &env).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let lines = loop {
            let (_, lines) = client.log(id, 0).unwrap();
            if lines.len() >= 3 || Instant::now() > deadline {
                break lines;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        // Its own variable, but the compositor's PATH.
        assert_eq!(lines[0], "Platinum", "{lines:?}");
        assert!(lines[1].ends_with("/sh"), "{lines:?}");
        assert!(patina_system::processes::spawn(" ", &[]).is_err());
    }
}
