//! Media players (ADR-0101): what's playing and its controls, through MPRIS
//! on the session bus. Browsers, UnAmp, Spotify and the rest each own an
//! `org.mpris.MediaPlayer2.*` name; the bar's media controls follow the one
//! playing. The calls wait for the players, so run them off the UI thread.

use std::collections::HashMap;

use zbus::blocking::Connection;
use zbus::blocking::proxy::Builder;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{OwnedValue, Value};

/// Every player's bus name starts with this.
const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

impl Status {
    fn parse(text: &str) -> Self {
        match text {
            "Playing" => Status::Playing,
            "Paused" => Status::Paused,
            _ => Status::Stopped,
        }
    }
}

/// A player and what it's on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Its name on the bus, which commands are sent to.
    pub bus_name: String,
    /// What it calls itself ("UnAmp", "Mozilla Firefox").
    pub identity: String,
    pub status: Status,
    /// The track's title, or the player's name when it gives none.
    pub title: String,
    pub artist: String,
    pub can_previous: bool,
    pub can_next: bool,
}

impl Player {
    /// The track and who plays it, for a tooltip.
    pub fn describe(&self) -> String {
        let by = if self.artist.is_empty() {
            self.identity.clone()
        } else {
            format!("{} · {}", self.artist, self.identity)
        };
        format!("{}\n{by}", self.title)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

impl Command {
    fn method(self) -> &'static str {
        match self {
            Command::PlayPause => "PlayPause",
            Command::Next => "Next",
            Command::Previous => "Previous",
        }
    }
}

pub fn connect() -> Result<Connection, String> {
    Connection::session().map_err(|e| format!("no session bus: {e}"))
}

/// The player to show: the first one playing, else the first paused. A
/// stopped one (a browser with nothing open) has nothing to control.
pub fn pick(players: Vec<Player>) -> Option<Player> {
    let mut paused = None;
    for p in players {
        match p.status {
            Status::Playing => return Some(p),
            Status::Paused if paused.is_none() => paused = Some(p),
            _ => {}
        }
    }
    paused
}

/// The players with something to control, for the volume menu and the bar's count (ADR-0117):
/// the ones playing first, then the paused, each in the order given. A stopped one (a browser
/// with nothing open) is left out.
pub fn active(players: Vec<Player>) -> Vec<Player> {
    let (mut playing, mut paused) = (Vec::new(), Vec::new());
    for p in players {
        match p.status {
            Status::Playing => playing.push(p),
            Status::Paused => paused.push(p),
            Status::Stopped => {}
        }
    }
    playing.extend(paused);
    playing
}

/// The text of a metadata entry: a string, or the first of a list of them
/// (`xesam:artist` is a list, though some players send a string).
fn text(metadata: &HashMap<String, OwnedValue>, key: &str) -> String {
    let Some(value) = metadata.get(key) else {
        return String::new();
    };
    let value: &Value = value;
    if let Ok(s) = <&str>::try_from(value) {
        return s.to_string();
    }
    if let Value::Array(items) = value {
        let names: Vec<String> = items
            .iter()
            .filter_map(|v| <&str>::try_from(v).ok().map(str::to_string))
            .collect();
        return names.join(", ");
    }
    String::new()
}

fn read(bus: &Connection, name: &str) -> Result<Player, String> {
    let proxy = |interface: &'static str| {
        Builder::<zbus::blocking::Proxy>::new(bus)
            .destination(name.to_string())
            .and_then(|b| b.path(PATH))
            .and_then(|b| b.interface(interface))
            .map(|b| b.cache_properties(CacheProperties::No))
            .and_then(|b| b.build())
            .map_err(|e| e.to_string())
    };
    let player = proxy(PLAYER)?;
    let get = |e: zbus::Error| e.to_string();
    let status: String = player.get_property("PlaybackStatus").map_err(get)?;
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap_or_default();
    let identity: String = proxy(ROOT)?
        .get_property("Identity")
        .unwrap_or_else(|_| name.trim_start_matches(PREFIX).to_string());
    let title = text(&metadata, "xesam:title");
    Ok(Player {
        bus_name: name.to_string(),
        title: if title.is_empty() {
            identity.clone()
        } else {
            title
        },
        identity,
        status: Status::parse(&status),
        artist: text(&metadata, "xesam:artist"),
        can_previous: player.get_property("CanGoPrevious").unwrap_or(true),
        can_next: player.get_property("CanGoNext").unwrap_or(true),
    })
}

/// Every player on the bus that answers, by name.
pub fn players(bus: &Connection) -> Result<Vec<Player>, String> {
    let dbus = zbus::blocking::fdo::DBusProxy::new(bus).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = dbus
        .list_names()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with(PREFIX))
        .collect();
    names.sort();
    // One that's gone, or won't answer, is left out, not an error.
    Ok(names.iter().filter_map(|n| read(bus, n).ok()).collect())
}

/// The player the bar shows, if any.
pub fn current(bus: &Connection) -> Result<Option<Player>, String> {
    players(bus).map(pick)
}

/// Sends `command` to the player at `bus_name`.
pub fn send(bus: &Connection, bus_name: &str, command: Command) -> Result<(), String> {
    let player = Builder::<zbus::blocking::Proxy>::new(bus)
        .destination(bus_name.to_string())
        .and_then(|b| b.path(PATH))
        .and_then(|b| b.interface(PLAYER))
        .map(|b| b.cache_properties(CacheProperties::No))
        .and_then(|b| b.build())
        .map_err(|e| e.to_string())?;
    player
        .call::<_, _, ()>(command.method(), &())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, status: Status) -> Player {
        Player {
            bus_name: format!("{PREFIX}{name}"),
            identity: name.into(),
            status,
            title: "Song".into(),
            artist: String::new(),
            can_previous: true,
            can_next: true,
        }
    }

    #[test]
    fn the_one_playing_is_shown_over_paused_ones() {
        let picked = pick(vec![
            player("a", Status::Paused),
            player("b", Status::Stopped),
            player("c", Status::Playing),
        ]);
        assert_eq!(picked.unwrap().identity, "c");
    }

    #[test]
    fn with_none_playing_the_first_paused_is_shown() {
        let picked = pick(vec![
            player("a", Status::Stopped),
            player("b", Status::Paused),
            player("c", Status::Paused),
        ]);
        assert_eq!(picked.unwrap().identity, "b");
    }

    #[test]
    fn active_players_are_the_playing_then_the_paused() {
        let names: Vec<String> = active(vec![
            player("a", Status::Paused),
            player("b", Status::Stopped),
            player("c", Status::Playing),
            player("d", Status::Paused),
            player("e", Status::Playing),
        ])
        .into_iter()
        .map(|p| p.identity)
        .collect();
        assert_eq!(names, ["c", "e", "a", "d"]);
    }

    #[test]
    fn stopped_players_have_nothing_to_show() {
        assert!(pick(vec![player("a", Status::Stopped)]).is_none());
        assert!(pick(Vec::new()).is_none());
    }

    #[test]
    fn artists_come_as_a_list_or_a_string() {
        let mut m = HashMap::new();
        let list = Value::from(vec!["Boards of Canada", "Another"]);
        m.insert(
            "xesam:artist".to_string(),
            OwnedValue::try_from(list).unwrap(),
        );
        assert_eq!(text(&m, "xesam:artist"), "Boards of Canada, Another");
        m.insert(
            "xesam:artist".to_string(),
            OwnedValue::try_from(Value::from("Solo")).unwrap(),
        );
        assert_eq!(text(&m, "xesam:artist"), "Solo");
        assert_eq!(text(&m, "xesam:title"), "");
    }

    #[test]
    fn the_tooltip_names_the_track_and_who_plays_it() {
        let mut p = player("UnAmp", Status::Playing);
        assert_eq!(p.describe(), "Song\nUnAmp");
        p.artist = "Artist".into();
        assert_eq!(p.describe(), "Song\nArtist · UnAmp");
    }
}
