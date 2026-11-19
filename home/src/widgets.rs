//! Pieces shared by the entity list and the dashboards.

use serde_json::Value;

use crate::ha::{Control, Entity};

/// A Home Assistant service to call, and what to show until the next poll confirms it.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceCall {
    pub domain: String,
    pub service: String,
    pub entity_id: String,
    /// Service data besides the entity id: `{ "brightness": 128 }`.
    pub data: Value,
    pub state: Option<String>,
    pub attributes: Vec<(String, Value)>,
}

impl ServiceCall {
    pub fn new(domain: &str, service: &str, entity_id: &str) -> Self {
        Self {
            domain: domain.into(),
            service: service.into(),
            entity_id: entity_id.into(),
            data: Value::Null,
            state: None,
            attributes: Vec::new(),
        }
    }

    pub fn state(mut self, state: &str) -> Self {
        self.state = Some(state.into());
        self
    }

    /// Also shown as the entity's attribute `name`, without being sent.
    pub fn attribute(mut self, name: &str, value: Value) -> Self {
        self.attributes.push((name.into(), value));
        self
    }

    /// Adds one service field, also shown as the entity's attribute of the same name (or `shown`).
    pub fn field(mut self, name: &str, value: Value, shown: Option<(&str, Value)>) -> Self {
        self.data[name] = value.clone();
        let (attribute, shown) = shown.map_or((name.to_string(), value), |(a, v)| (a.into(), v));
        self.attributes.push((attribute, shown));
        self
    }
}

/// What a click in the entity list or a dashboard asks the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Call(ServiceCall),
    /// Open an entity's own window: a light's settings, a sensor's graph.
    Open(String),
}

/// An entity's name, its emoji in colour (Home Assistant names often have one), weak when it
/// is unavailable, and clickable if `clickable`.
pub fn name_label(ui: &mut egui::Ui, name: &str, weak: bool, clickable: bool) -> egui::Response {
    let colour = if weak { ui.visuals().weak_text_color() } else { ui.visuals().text_color() };
    let line = patina::emoji::line(
        ui.painter(),
        name,
        egui::TextStyle::Body.resolve(ui.style()),
        colour,
        f32::INFINITY,
    );
    let sense = if clickable { egui::Sense::click() } else { egui::Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(line.size(), sense);
    line.paint(ui.painter(), rect.min);
    response
}

/// One entity: its name, state and control. What the user does is pushed to `events`.
pub fn entity_row(ui: &mut egui::Ui, e: &Entity, name: Option<&str>, events: &mut Vec<Event>) {
    ui.horizontal(|ui| {
        let name = name.unwrap_or_else(|| e.name());
        let opens = if e.domain() == "light" && !e.is_unavailable() {
            Some("Light settings")
        } else {
            crate::chart::is_graphable(e).then_some("History")
        };
        if let Some(hint) = opens {
            // A light's name opens its settings, and a sensor's its graph.
            let label = name_label(ui, name, e.is_unavailable(), true);
            if label.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint).clicked() {
                events.push(Event::Open(e.id.clone()));
            }
        } else {
            name_label(ui, name, e.is_unavailable(), false);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match e.control() {
            Control::Toggle(on) => {
                let mut value = on;
                if ui.add(egui::Checkbox::without_text(&mut value)).changed()
                    && let Some((domain, service, state)) = e.toggle_call(value)
                {
                    events.push(Event::Call(ServiceCall::new(domain, service, &e.id).state(state)));
                }
                ui.weak(e.display_state());
            }
            Control::Press => {
                if ui.button("Run").clicked()
                    && let Some((domain, service)) = e.press_call()
                {
                    events.push(Event::Call(ServiceCall::new(domain, service, &e.id)));
                }
            }
            Control::None => {
                ui.weak(e.display_state());
            }
        });
    });
}
