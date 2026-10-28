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
    Call { domain: String, service: String, entity_id: String, data: Value },
}

pub enum Update {
    States(Vec<Entity>),
    Dashboards(Vec<Dashboard>),
    Error(String),
}

/// One of the user's Lovelace dashboards.
#[derive(Debug, Clone)]
pub struct Dashboard {
    /// The dashboard's path in its URL; `None` for the default one.
    pub url_path: Option<String>,
    pub title: String,
    /// The Lovelace config (`{ views: [...] }`), or why there isn't one.
    pub config: Result<Value, String>,
}

pub struct Client {
    tx: Sender<Command>,
    pub rx: Receiver<Update>,
    updates: Sender<Update>,
    account: Account,
    ctx: egui::Context,
}

impl Client {
    /// Starts the worker. It stops when the `Client` is dropped.
    pub fn start(ctx: &egui::Context, account: Account) -> Self {
        let (tx, commands) = mpsc::channel();
        let (updates, rx) = mpsc::channel();
        let client = Self { tx, rx, updates: updates.clone(), account: account.clone(), ctx: ctx.clone() };
        let ctx = ctx.clone();
        std::thread::spawn(move || worker(account, commands, updates, ctx));
        client.reload_dashboards();
        client
    }

    /// Fetches the dashboards again, on a thread of their own (ADR-0004).
    pub fn reload_dashboards(&self) {
        let (account, updates, ctx) = (self.account.clone(), self.updates.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let update = match fetch_dashboards(&account) {
                Ok(dashboards) => Update::Dashboards(dashboards),
                Err(e) => Update::Error(format!("Dashboards: {e}")),
            };
            let _ = updates.send(update);
            ctx.request_repaint();
        });
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

/// `http://host:8123/` → `ws://host:8123/api/websocket`, and https → wss.
fn websocket_url(account: &Account) -> Result<String, String> {
    let url = account.endpoint("/api/websocket");
    if let Some(rest) = url.strip_prefix("https://") {
        Ok(format!("wss://{rest}"))
    } else if let Some(rest) = url.strip_prefix("http://") {
        Ok(format!("ws://{rest}"))
    } else {
        Err("The address must start with http:// or https://".into())
    }
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn read_json(socket: &mut Socket) -> Result<Value, String> {
    loop {
        match socket.read().map_err(|e| e.to_string())? {
            tungstenite::Message::Text(text) => return serde_json::from_str(text.as_str()).map_err(|e| e.to_string()),
            tungstenite::Message::Close(_) => return Err("The server closed the connection".into()),
            _ => {}
        }
    }
}

/// Sends one command and waits for its result; `Err` carries the server's message.
fn request(socket: &mut Socket, id: u64, mut command: Value) -> Result<Value, String> {
    command["id"] = id.into();
    socket.send(tungstenite::Message::text(command.to_string())).map_err(|e| e.to_string())?;
    loop {
        let reply = read_json(socket)?;
        if reply["id"].as_u64() != Some(id) {
            continue;
        }
        return if reply["success"].as_bool() == Some(true) {
            Ok(reply["result"].clone())
        } else {
            Err(reply["error"]["message"].as_str().unwrap_or("failed").to_string())
        };
    }
}

/// Connects and logs in over the WebSocket API, the only place Lovelace configs are served.
fn fetch_dashboards(account: &Account) -> Result<Vec<Dashboard>, String> {
    let (mut socket, _) = tungstenite::connect(websocket_url(account)?).map_err(|e| e.to_string())?;
    if read_json(&mut socket)?["type"] != "auth_required" {
        return Err("Not a Home Assistant WebSocket".into());
    }
    socket
        .send(tungstenite::Message::text(
            serde_json::json!({ "type": "auth", "access_token": account.token.trim() }).to_string(),
        ))
        .map_err(|e| e.to_string())?;
    if read_json(&mut socket)?["type"] != "auth_ok" {
        return Err("Home Assistant refused the access token".into());
    }

    let mut id = 0;
    let mut next = || {
        id += 1;
        id
    };
    let mut paths: Vec<(Option<String>, String)> = vec![(None, "Overview".into())];
    for d in request(&mut socket, next(), serde_json::json!({ "type": "lovelace/dashboards/list" }))?
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(path) = d["url_path"].as_str() {
            paths.push((Some(path.into()), d["title"].as_str().unwrap_or(path).into()));
        }
    }
    let mut dashboards = Vec::new();
    for (url_path, title) in paths {
        let command = serde_json::json!({ "type": "lovelace/config", "url_path": url_path });
        let config = request(&mut socket, next(), command);
        // An auto-generated dashboard has no stored config: nothing to draw.
        if url_path.is_none() && config.as_ref().is_err_and(|e| e.contains("not found")) {
            continue;
        }
        dashboards.push(Dashboard { url_path, title, config });
    }
    let _ = socket.close(None);
    Ok(dashboards)
}

fn call(
    agent: &ureq::Agent,
    account: &Account,
    domain: &str,
    service: &str,
    entity_id: &str,
    data: Value,
) -> Result<(), String> {
    let mut body = if data.is_object() { data } else { serde_json::json!({}) };
    body["entity_id"] = entity_id.into();
    agent
        .post(&account.endpoint(&format!("/api/services/{domain}/{service}")))
        .header("Authorization", &format!("Bearer {}", account.token.trim()))
        .send_json(body)
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
            Ok(Command::Call { domain, service, entity_id, data }) => {
                if let Err(e) = call(&agent, &account, &domain, &service, &entity_id, data) {
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
    let mut entities = vec![
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
    ];
    // The living room light is a colour light with effects; the bedroom one only dims and warms.
    let attributes = serde_json::json!({
        "light.living_room": {
            "supported_color_modes": ["color_temp", "xy"], "brightness": 180, "color_mode": "xy",
            "min_color_temp_kelvin": 2000, "max_color_temp_kelvin": 6535, "hs_color": [28.0, 74.0],
            "effect_list": ["gradient", "candle", "fireplace"], "effect": "gradient",
        },
        "light.bedroom": {
            "supported_color_modes": ["color_temp"], "brightness": 90, "color_mode": "color_temp",
            "min_color_temp_kelvin": 2200, "max_color_temp_kelvin": 6500, "color_temp_kelvin": 2700,
        },
    });
    for entity in &mut entities {
        if let Some(extra) = attributes[entity.id.as_str()].as_object() {
            entity.attributes.extend(extra.clone());
        }
    }
    entities
}

/// A made-up dashboard to match `demo()`.
pub fn demo_dashboards() -> Vec<Dashboard> {
    let config = serde_json::json!({ "views": [
        { "title": "Home", "type": "sections", "sections": [
            { "cards": [
                { "type": "heading", "heading": "Living" },
                { "type": "tile", "entity": "light.living_room" },
                { "type": "tile", "entity": "scene.movie_night" },
                { "type": "tile", "entity": "light.bedroom", "name": "Bedroom Lamp" },
            ]},
            { "cards": [
                { "type": "heading", "heading": "Climate" },
                { "type": "glance", "title": "Temperatures", "entities": [
                    { "entity": "sensor.living_room_temperature", "name": "Inside" },
                    { "entity": "sensor.outside_temperature", "name": "Outside" },
                    "sensor.power",
                ]},
                { "type": "entities", "title": "Switches", "entities": [
                    "switch.coffee_machine", "switch.office_fan",
                ]},
            ]},
            { "cards": [
                { "type": "heading", "heading": "Security" },
                { "type": "horizontal-stack", "cards": [
                    { "type": "button", "entity": "lock.front_door" },
                    { "type": "button", "entity": "cover.garage", "name": "Garage" },
                ]},
                { "type": "markdown", "content": "Goodnight runs the **bedtime** script." },
                { "type": "button", "entity": "script.goodnight" },
            ]},
        ]},
        { "title": "Rooms", "cards": [
            { "type": "entities", "title": "Kitchen", "entities": ["light.kitchen", "switch.coffee_machine"] },
            { "type": "vertical-stack", "cards": [
                { "type": "entity", "entity": "sensor.power" },
                { "type": "gauge", "entity": "sensor.power" },
            ]},
        ]},
    ]});
    vec![Dashboard { url_path: None, title: "Overview".into(), config: Ok(config) }]
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

    #[test]
    fn websocket_url_follows_the_scheme() {
        let account = |url: &str| Account { url: url.into(), token: "t".into() };
        assert_eq!(websocket_url(&account("http://ha.local:8123/")).unwrap(), "ws://ha.local:8123/api/websocket");
        assert_eq!(websocket_url(&account("https://ha.example.com")).unwrap(), "wss://ha.example.com/api/websocket");
        assert!(websocket_url(&account("ha.local")).is_err());
    }

    /// A stand-in Home Assistant that speaks the WebSocket protocol for one connection.
    fn fake_server(token: &'static str) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            let send = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>, v: Value| {
                ws.send(tungstenite::Message::text(v.to_string())).unwrap();
            };
            let recv = |ws: &mut tungstenite::WebSocket<std::net::TcpStream>| -> Value {
                serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap()
            };
            send(&mut ws, serde_json::json!({ "type": "auth_required" }));
            let auth = recv(&mut ws);
            if auth["access_token"] != token {
                send(&mut ws, serde_json::json!({ "type": "auth_invalid" }));
                return;
            }
            send(&mut ws, serde_json::json!({ "type": "auth_ok" }));
            while let Ok(message) = ws.read() {
                let Ok(text) = message.to_text() else { return };
                let Ok(command) = serde_json::from_str::<Value>(text) else { return };
                let id = command["id"].clone();
                let reply = match (command["type"].as_str(), command["url_path"].as_str()) {
                    (Some("lovelace/dashboards/list"), _) => serde_json::json!({
                        "id": id, "type": "result", "success": true,
                        "result": [{ "url_path": "energy", "title": "Energy" }]
                    }),
                    (Some("lovelace/config"), None) => serde_json::json!({
                        "id": id, "type": "result", "success": false,
                        "error": { "code": "config_not_found", "message": "Config not found." }
                    }),
                    (Some("lovelace/config"), Some(_)) => serde_json::json!({
                        "id": id, "type": "result", "success": true, "result": { "views": [] }
                    }),
                    _ => return,
                };
                send(&mut ws, reply);
            }
        });
        url
    }

    #[test]
    fn fetches_dashboards_and_skips_an_auto_generated_default() {
        let account = Account { url: fake_server("secret"), token: "secret".into() };
        let dashboards = fetch_dashboards(&account).unwrap();
        assert_eq!(dashboards.len(), 1);
        assert_eq!((dashboards[0].url_path.as_deref(), dashboards[0].title.as_str()), (Some("energy"), "Energy"));
        assert!(dashboards[0].config.is_ok());
    }

    #[test]
    fn a_wrong_token_is_reported() {
        let account = Account { url: fake_server("secret"), token: "nope".into() };
        assert_eq!(fetch_dashboards(&account).unwrap_err(), "Home Assistant refused the access token");
    }
}
