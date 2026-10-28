//! The Home Assistant REST API: entity states and service calls, on a worker
//! thread so the UI never waits on the network (ADR-0003).

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// How often the states are fetched again.
const POLL: Duration = Duration::from_secs(5);

/// Where Home Assistant is and the long-lived access token to use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub url: String,
    pub token: String,
}

impl Account {
    pub fn is_set(&self) -> bool {
        !self.url.trim().is_empty() && !self.token.trim().is_empty()
    }

    /// `http://host:8123` plus a path, whether or not the URL ends in a slash.
    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.url.trim().trim_end_matches('/'))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub id: String,
    pub state: String,
    pub attributes: Map<String, Value>,
}

/// What the user can do to an entity from its row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// A switch showing whether it's on.
    Toggle(bool),
    /// A button that runs it (a scene, a script).
    Press,
    /// Shown, not controlled.
    None,
}

impl Entity {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            id: v.get("entity_id")?.as_str()?.to_string(),
            state: v.get("state")?.as_str()?.to_string(),
            attributes: v.get("attributes").and_then(Value::as_object).cloned().unwrap_or_default(),
        })
    }

    pub fn domain(&self) -> &str {
        self.id.split_once('.').map_or(&self.id, |(d, _)| d)
    }

    pub fn name(&self) -> &str {
        self.attributes.get("friendly_name").and_then(Value::as_str).unwrap_or(&self.id)
    }

    /// The state with its unit, as a sensor shows it: "21.5 °C".
    pub fn display_state(&self) -> String {
        match self.attributes.get("unit_of_measurement").and_then(Value::as_str) {
            Some(unit) if !unit.is_empty() => format!("{} {unit}", self.state),
            _ => self.state.clone(),
        }
    }

    pub fn is_unavailable(&self) -> bool {
        self.state == "unavailable" || self.state == "unknown"
    }

    pub fn control(&self) -> Control {
        if self.state == "unavailable" {
            return Control::None;
        }
        match self.domain() {
            "light" | "switch" | "input_boolean" | "fan" | "automation" => Control::Toggle(self.state == "on"),
            "cover" => Control::Toggle(self.state == "open"),
            "lock" => Control::Toggle(self.state == "unlocked"),
            "scene" | "script" | "button" | "input_button" => Control::Press,
            _ => Control::None,
        }
    }

    /// The service that flips a toggle to `on`, and the state to show until the next poll.
    pub fn toggle_call(&self, on: bool) -> Option<(&'static str, &'static str, &'static str)> {
        let (domain, service, state) = match (self.domain(), on) {
            ("cover", true) => ("cover", "open_cover", "open"),
            ("cover", false) => ("cover", "close_cover", "closed"),
            ("lock", true) => ("lock", "unlock", "unlocked"),
            ("lock", false) => ("lock", "lock", "locked"),
            (_, true) => ("homeassistant", "turn_on", "on"),
            (_, false) => ("homeassistant", "turn_off", "off"),
        };
        matches!(self.control(), Control::Toggle(_)).then_some((domain, service, state))
    }

    pub fn press_call(&self) -> Option<(&'static str, &'static str)> {
        match self.domain() {
            "scene" => Some(("scene", "turn_on")),
            "script" => Some(("script", "turn_on")),
            "button" => Some(("button", "press")),
            "input_button" => Some(("input_button", "press")),
            _ => None,
        }
    }
}

pub enum Command {
    Refresh,
    Call { domain: String, service: String, entity_id: String },
}

pub enum Update {
    States(Vec<Entity>),
    Error(String),
}

pub struct Client {
    tx: Sender<Command>,
    pub rx: Receiver<Update>,
}

impl Client {
    /// Starts the worker. It stops when the `Client` is dropped.
    pub fn start(ctx: &egui::Context, account: Account) -> Self {
        let (tx, commands) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || worker(account, commands, updates, ctx));
        Self { tx, rx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).build().new_agent()
}

fn fetch(agent: &ureq::Agent, account: &Account) -> Result<Vec<Entity>, String> {
    let body: Value = agent
        .get(&account.endpoint("/api/states"))
        .header("Authorization", &format!("Bearer {}", account.token.trim()))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let mut entities: Vec<Entity> =
        body.as_array().ok_or("unexpected reply")?.iter().filter_map(Entity::from_json).collect();
    entities.sort_by_key(|e| e.name().to_lowercase());
    Ok(entities)
}

fn call(agent: &ureq::Agent, account: &Account, domain: &str, service: &str, entity_id: &str) -> Result<(), String> {
    agent
        .post(&account.endpoint(&format!("/api/services/{domain}/{service}")))
        .header("Authorization", &format!("Bearer {}", account.token.trim()))
        .send_json(serde_json::json!({ "entity_id": entity_id }))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn worker(account: Account, commands: Receiver<Command>, updates: Sender<Update>, ctx: egui::Context) {
    let agent = agent();
    let mut refresh = true;
    loop {
        if refresh {
            let update = match fetch(&agent, &account) {
                Ok(entities) => Update::States(entities),
                Err(e) => Update::Error(e),
            };
            if updates.send(update).is_err() {
                return;
            }
            ctx.request_repaint();
        }
        refresh = true;
        match commands.recv_timeout(POLL) {
            Ok(Command::Refresh) | Err(RecvTimeoutError::Timeout) => {}
            Ok(Command::Call { domain, service, entity_id }) => {
                if let Err(e) = call(&agent, &account, &domain, &service, &entity_id) {
                    if updates.send(Update::Error(e)).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Made-up entities for screenshots and trying the app without a server.
pub fn demo() -> Vec<Entity> {
    let e = |id: &str, state: &str, name: &str, unit: Option<&str>| {
        let mut attributes = Map::new();
        attributes.insert("friendly_name".into(), name.into());
        if let Some(unit) = unit {
            attributes.insert("unit_of_measurement".into(), unit.into());
        }
        Entity { id: id.into(), state: state.into(), attributes }
    };
    vec![
        e("light.living_room", "on", "Living Room", None),
        e("light.kitchen", "off", "Kitchen", None),
        e("light.bedroom", "on", "Bedroom", None),
        e("switch.coffee_machine", "off", "Coffee Machine", None),
        e("switch.office_fan", "on", "Office Fan", None),
        e("lock.front_door", "locked", "Front Door", None),
        e("cover.garage", "closed", "Garage Door", None),
        e("scene.movie_night", "scening", "Movie Night", None),
        e("script.goodnight", "off", "Goodnight", None),
        e("sensor.living_room_temperature", "21.5", "Living Room Temperature", Some("°C")),
        e("sensor.outside_temperature", "7.2", "Outside Temperature", Some("°C")),
        e("sensor.power", "412", "Power", Some("W")),
        e("binary_sensor.front_door_contact", "off", "Front Door Contact", None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(json: Value) -> Entity {
        Entity::from_json(&json).unwrap()
    }

    #[test]
    fn parses_a_state() {
        let e = entity(serde_json::json!({
            "entity_id": "sensor.temp", "state": "21.5",
            "attributes": { "friendly_name": "Temp", "unit_of_measurement": "°C" }
        }));
        assert_eq!((e.domain(), e.name()), ("sensor", "Temp"));
        assert_eq!(e.display_state(), "21.5 °C");
        assert_eq!(e.control(), Control::None);
    }

    #[test]
    fn name_falls_back_to_the_id() {
        let e = entity(serde_json::json!({ "entity_id": "light.x", "state": "on" }));
        assert_eq!(e.name(), "light.x");
    }

    #[test]
    fn lights_toggle_through_homeassistant() {
        let e = entity(serde_json::json!({ "entity_id": "light.x", "state": "off" }));
        assert_eq!(e.control(), Control::Toggle(false));
        assert_eq!(e.toggle_call(true), Some(("homeassistant", "turn_on", "on")));
    }

    #[test]
    fn locks_and_covers_use_their_own_services() {
        let lock = entity(serde_json::json!({ "entity_id": "lock.d", "state": "locked" }));
        assert_eq!(lock.control(), Control::Toggle(false));
        assert_eq!(lock.toggle_call(true), Some(("lock", "unlock", "unlocked")));
        let cover = entity(serde_json::json!({ "entity_id": "cover.g", "state": "open" }));
        assert_eq!(cover.toggle_call(false), Some(("cover", "close_cover", "closed")));
    }

    #[test]
    fn unavailable_entities_have_no_controls() {
        let e = entity(serde_json::json!({ "entity_id": "light.x", "state": "unavailable" }));
        assert_eq!((e.control(), e.toggle_call(true)), (Control::None, None));
    }

    #[test]
    fn scenes_are_pressed() {
        let e = entity(serde_json::json!({ "entity_id": "scene.s", "state": "unknown" }));
        assert_eq!((e.control(), e.press_call()), (Control::Press, Some(("scene", "turn_on")))); // never activated
        let e = entity(serde_json::json!({ "entity_id": "scene.s", "state": "2026-01-01T00:00:00" }));
        assert_eq!((e.control(), e.press_call()), (Control::Press, Some(("scene", "turn_on"))));
    }

    #[test]
    fn endpoint_ignores_a_trailing_slash() {
        let a = Account { url: " http://ha.local:8123/ ".into(), token: "t".into() };
        assert_eq!(a.endpoint("/api/states"), "http://ha.local:8123/api/states");
    }
}
