//! The open windows and the process each belongs to, from the compositor's
//! `com.sectorflabs.Patina.Tabs` (ADR-0088), for the process manager: which
//! processes are apps, and bringing one's window forward.

use crate::pins::{NAME, PATH};

/// A window, as the compositor lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub app_id: String,
    pub title: String,
    pub pid: u32,
}

/// A connection to the compositor's window list, kept for polling.
pub struct Windows {
    proxy: zbus::blocking::Proxy<'static>,
}

impl Windows {
    pub fn connect() -> Result<Self, String> {
        let bus = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
        let proxy =
            zbus::blocking::Proxy::new_owned(bus, NAME, PATH, NAME).map_err(|e| e.to_string())?;
        Ok(Self { proxy })
    }

    /// The open windows, in the bar's order.
    pub fn list(&self) -> Result<Vec<Window>, String> {
        let windows: Vec<(String, String, u32)> =
            self.proxy.call("Windows", &()).map_err(|e| e.to_string())?;
        Ok(windows
            .into_iter()
            .map(|(app_id, title, pid)| Window { app_id, title, pid })
            .collect())
    }

    /// Brings forward a window of process `pid`.
    pub fn activate(&self, pid: u32) -> Result<(), String> {
        self.proxy
            .call::<_, _, ()>("Activate", &(pid,))
            .map_err(|e| e.to_string())
    }
}
