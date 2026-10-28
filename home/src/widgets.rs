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
    /// Open an entity's settings (a light's colour and brightness).
    Open(String),
}

/// One entity: its name, state and control. What the user does is pushed to `events`.
pub fn entity_row(ui: &mut egui::Ui, e: &Entity, name: Option<&str>, events: &mut Vec<Event>) {
    ui.horizontal(|ui| {
        let name = egui::RichText::new(name.unwrap_or_else(|| e.name()));
        let name = if e.is_unavailable() { name.weak() } else { name };
        if e.domain() == "light" && !e.is_unavailable() {
            // A light's name opens its settings.
            let label = ui.add(egui::Label::new(name).sense(egui::Sense::click()));
            if label.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Light settings").clicked() {
                events.push(Event::Open(e.id.clone()));
            }
        } else {
            ui.label(name);
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
