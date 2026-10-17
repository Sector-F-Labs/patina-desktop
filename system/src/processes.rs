//! The desktop's processes, from outside the compositor (ADR-0045): what it
//! started, their state and logs, and restarting or stopping them, through
//! its `com.sectorflabs.Patina.Processes` on the session bus. For Settings'
//! Processes page.

use std::time::{Duration, SystemTime};

use zbus::blocking::{Connection, Proxy};

const NAME: &str = "com.sectorflabs.Patina.Processes";
const PATH: &str = "/com/sectorflabs/Patina/Processes";

/// One of the processes the compositor started.
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub id: u32,
    /// Its program's file name.
    pub name: String,
    pub command: String,
    /// `None` when it isn't running.
    pub pid: Option<u32>,
    /// "running", "exited with 1", "stopped", "gave up: …".
    pub status: String,
    /// Started again whenever it exits: the wallpaper and the bar.
    pub kept: bool,
    pub restarts: u32,
    /// When it last started.
    pub started: SystemTime,
}

impl Process {
    pub fn running(&self) -> bool {
        self.pid.is_some()
    }
}

/// A connection to the session bus, for asking the compositor.
pub struct Processes {
    bus: Connection,
}

type Row = (u32, String, String, u32, String, bool, u32, u64);

impl Processes {
    pub fn connect() -> Result<Self, String> {
        let bus = Connection::session().map_err(|e| format!("no session bus: {e}"))?;
        Ok(Self { bus })
    }

    fn proxy(&self) -> Result<Proxy<'_>, String> {
        Proxy::new(&self.bus, NAME, PATH, NAME).map_err(|e| e.to_string())
    }

    /// Whether the compositor serves the list: not under another desktop.
    pub fn served(&self) -> Result<bool, String> {
        let dbus = zbus::blocking::fdo::DBusProxy::new(&self.bus).map_err(|e| e.to_string())?;
        let name = zbus::names::BusName::try_from(NAME).map_err(|e| e.to_string())?;
        dbus.name_has_owner(name).map_err(|e| e.to_string())
    }

    /// Everything the compositor started and still lists, oldest first.
    pub fn list(&self) -> Result<Vec<Process>, String> {
        let rows: Vec<Row> = self.proxy()?.call("List", &()).map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(
                |(id, name, command, pid, status, kept, restarts, started)| Process {
                    id,
                    name,
                    command,
                    pid: (pid != 0).then_some(pid),
                    status,
                    kept,
                    restarts,
                    started: SystemTime::UNIX_EPOCH + Duration::from_secs(started),
                },
            )
            .collect())
    }

    /// Stops process `id` and starts it again, or starts it if it's down.
    pub fn restart(&self, id: u32) -> Result<(), String> {
        self.proxy()?
            .call::<_, _, ()>("Restart", &(id,))
            .map_err(|e| e.to_string())
    }

    /// Stops process `id`; the wallpaper and bar stay down until restarted.
    pub fn stop(&self, id: u32) -> Result<(), String> {
        self.proxy()?
            .call::<_, _, ()>("Stop", &(id,))
            .map_err(|e| e.to_string())
    }

    /// Process `id`'s output from line `from` on, and the line to ask from
    /// next.
    pub fn log(&self, id: u32, from: u64) -> Result<(u64, Vec<String>), String> {
        self.proxy()?
            .call("Log", &(id, from))
            .map_err(|e| e.to_string())
    }
}
