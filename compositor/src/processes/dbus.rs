//! The desktop's processes on the session bus (ADR-0045), for Settings'
//! Processes page: `com.sectorflabs.Patina.Processes` at
//! `/com/sectorflabs/Patina/Processes`. The list is global, so the
//! interface reads and changes it from zbus's threads, with no trip
//! through the event loop. There are no signals; readers poll.

pub const NAME: &str = "com.sectorflabs.Patina.Processes";
pub const PATH: &str = "/com/sectorflabs/Patina/Processes";

/// Claims the name and serves the interface while the connection lives.
pub fn serve() -> Result<zbus::blocking::Connection, String> {
    zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(NAME))
        .and_then(|b| b.serve_at(PATH, Interface))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())
}

struct Interface;

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

    fn restart(&self, id: u32) -> zbus::fdo::Result<()> {
        super::restart(id).map_err(failed)
    }

    fn stop(&self, id: u32) -> zbus::fdo::Result<()> {
        super::stop(id).map_err(failed)
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
        let _served = super::serve().expect("the name is free on a bus of our own");
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
    }
}
