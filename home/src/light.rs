//! A light's settings, as Home Assistant's own dashboard offers when you click one (ADR-0005):
//! power, brightness, colour temperature, a colour and effects, for what the light supports.

use serde_json::{Value, json};

use crate::ha::Entity;
use crate::widgets::{Event, ServiceCall};

/// What a light supports and its current settings, read from the entity's attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Light {
    pub on: bool,
    pub brightness: bool,
    pub color_temp: bool,
    pub color: bool,
    /// The coldest and warmest the light goes, in kelvin.
    pub kelvin: (f32, f32),
    pub effects: Vec<String>,
    pub effect: Option<String>,
    /// Brightness in percent; 0 when off.
    pub percent: f32,
    pub current_kelvin: Option<f32>,
    /// Hue (0–360) and saturation (0–100).
    pub hs: Option<[f32; 2]>,
}

impl Light {
    pub fn of(e: &Entity) -> Self {
        let a = &e.attributes;
        let modes: Vec<&str> = a
            .get("supported_color_modes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let number = |name: &str| a.get(name).and_then(Value::as_f64).map(|n| n as f32);
        Self {
            on: e.state == "on",
            brightness: modes.iter().any(|m| *m != "onoff"),
            color_temp: modes.contains(&"color_temp"),
            color: modes.iter().any(|m| matches!(*m, "hs" | "xy" | "rgb" | "rgbw" | "rgbww")),
            kelvin: (
                number("min_color_temp_kelvin").unwrap_or(2000.0),
                number("max_color_temp_kelvin").unwrap_or(6500.0),
            ),
            effects: a
                .get("effect_list")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect(),
            effect: a.get("effect").and_then(Value::as_str).map(String::from),
            percent: number("brightness").map_or(0.0, |b| (b / 255.0 * 100.0).round()),
            current_kelvin: number("color_temp_kelvin"),
            hs: a
                .get("hs_color")
                .and_then(Value::as_array)
                .and_then(|c| Some([c.first()?.as_f64()? as f32, c.get(1)?.as_f64()? as f32])),
        }
    }
}

/// The light's open settings window. A slider keeps its value while dragged, so a poll doesn't
/// snap it back; the value is sent when it's let go.
pub struct LightDialog {
    pub id: String,
    brightness: Option<f32>,
    kelvin: Option<f32>,
    hs: Option<[f32; 2]>,
}

impl LightDialog {
    pub fn new(id: String) -> Self {
        Self { id, brightness: None, kelvin: None, hs: None }
    }

    /// Draws the window; false once it's closed.
    pub fn show(&mut self, ctx: &egui::Context, e: &Entity, events: &mut Vec<Event>) -> bool {
        let light = Light::of(e);
        let mut open = true;
        egui::Window::new(e.name())
            .id(egui::Id::new("light-settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| self.contents(ui, e, &light, events));
        open && !ctx.input(|i| i.key_pressed(egui::Key::Escape))
    }

    fn contents(&mut self, ui: &mut egui::Ui, e: &Entity, light: &Light, events: &mut Vec<Event>) {
        ui.spacing_mut().slider_width = 240.0;
        let turn_on = || ServiceCall::new("light", "turn_on", &e.id).state("on");

        let mut on = light.on;
        if ui.checkbox(&mut on, "On").changed()
            && let Some((domain, service, state)) = e.toggle_call(on)
        {
            events.push(Event::Call(ServiceCall::new(domain, service, &e.id).state(state)));
        }

        if light.brightness {
            let mut value = self.brightness.unwrap_or(light.percent);
            let r = ui.add(egui::Slider::new(&mut value, 1.0..=100.0).suffix("%").fixed_decimals(0).text("Brightness"));
            if let Some(done) = finished(&r, &mut self.brightness, value) {
                let raw = json!((done * 255.0 / 100.0).round());
                events.push(Event::Call(turn_on().field("brightness", raw, None)));
            }
        }

        if light.color_temp {
            let (min, max) = light.kelvin;
            let mut value = self.kelvin.unwrap_or(light.current_kelvin.unwrap_or((min + max) / 2.0));
            let r = ui.add(egui::Slider::new(&mut value, min..=max).suffix(" K").fixed_decimals(0).text("Temperature"));
            warm_to_cool_bar(ui);
            if let Some(done) = finished(&r, &mut self.kelvin, value) {
                let call = turn_on().field("color_temp_kelvin", json!(done.round()), None);
                events.push(Event::Call(call.attribute("color_mode", "color_temp".into())));
            }
        }

        if light.color {
            ui.add_space(6.0);
            let mut hs = self.hs.or(light.hs).unwrap_or([0.0, 0.0]);
            let r = hue_saturation(ui, &mut hs);
            if let Some(done) = finished(&r, &mut self.hs, hs) {
                let call = turn_on().field("hs_color", json!([done[0].round(), done[1].round()]), None);
                events.push(Event::Call(call.attribute("color_mode", "hs".into())));
            }
        }

        if !light.effects.is_empty() {
            ui.add_space(6.0);
            let mut chosen = None;
            egui::ComboBox::from_label("Effect").selected_text(light.effect.as_deref().unwrap_or("None")).show_ui(
                ui,
                |ui| {
                    for effect in &light.effects {
                        if ui.selectable_label(light.effect.as_deref() == Some(effect), effect).clicked() {
                            chosen = Some(effect.clone());
                        }
                    }
                },
            );
            if let Some(effect) = chosen {
                events.push(Event::Call(turn_on().field("effect", json!(effect), None)));
            }
        }
    }
}

/// While a control is being dragged its value is kept in `pending`; returns the value once it's let
/// go, or changed some other way (a click, the keyboard).
fn finished<T: Copy>(r: &egui::Response, pending: &mut Option<T>, value: T) -> Option<T> {
    if r.changed() {
        *pending = Some(value);
    }
    if r.drag_stopped() || (r.changed() && !r.dragged()) {
        return pending.take();
    }
    None
}

/// A colour strip from warm to cool white, above the temperature slider.
fn warm_to_cool_bar(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.spacing().slider_width, 6.0), egui::Sense::hover());
    let mut mesh = egui::Mesh::default();
    let (warm, cool) = (egui::Color32::from_rgb(255, 170, 80), egui::Color32::from_rgb(170, 205, 255));
    mesh.colored_vertex(rect.left_top(), warm);
    mesh.colored_vertex(rect.right_top(), cool);
    mesh.colored_vertex(rect.right_bottom(), cool);
    mesh.colored_vertex(rect.left_bottom(), warm);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Hue along the width, saturation up the height (white at the bottom). Brightness is its own slider.
fn hue_saturation(ui: &mut egui::Ui, hs: &mut [f32; 2]) -> egui::Response {
    let size = egui::vec2(ui.spacing().slider_width + 60.0, 110.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    if let Some(p) = response.interact_pointer_pos() {
        *hs = [
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * 360.0,
            (1.0 - (p.y - rect.top()) / rect.height()).clamp(0.0, 1.0) * 100.0,
        ];
        response.mark_changed();
    }
    let (nx, ny) = (36u32, 6u32);
    let mut mesh = egui::Mesh::default();
    for iy in 0..=ny {
        for ix in 0..=nx {
            let (fx, fy) = (ix as f32 / nx as f32, iy as f32 / ny as f32);
            let pos = egui::pos2(rect.left() + fx * rect.width(), rect.top() + fy * rect.height());
            mesh.colored_vertex(pos, egui::ecolor::Hsva::new(fx, 1.0 - fy, 1.0, 1.0).into());
        }
    }
    for iy in 0..ny {
        for ix in 0..nx {
            let i = iy * (nx + 1) + ix;
            mesh.add_triangle(i, i + 1, i + nx + 2);
            mesh.add_triangle(i, i + nx + 2, i + nx + 1);
        }
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    let marker =
        egui::pos2(rect.left() + hs[0] / 360.0 * rect.width(), rect.top() + (1.0 - hs[1] / 100.0) * rect.height());
    ui.painter().circle(
        marker,
        6.0,
        egui::ecolor::Hsva::new(hs[0] / 360.0, hs[1] / 100.0, 1.0, 1.0),
        egui::Stroke::new(2.0, egui::Color32::BLACK),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(json: Value) -> Entity {
        let mut e = Entity { id: "light.x".into(), state: "on".into(), attributes: Default::default() };
        e.attributes = json.as_object().unwrap().clone();
        e
    }

    #[test]
    fn reads_what_a_colour_light_supports() {
        let l = Light::of(&light(json!({
            "supported_color_modes": ["color_temp", "xy"], "brightness": 128,
            "min_color_temp_kelvin": 2000, "max_color_temp_kelvin": 6535,
            "hs_color": [30.5, 80.0], "effect_list": ["gradient", "candle"], "effect": "candle",
        })));
        assert!(l.on && l.brightness && l.color_temp && l.color);
        assert_eq!((l.percent, l.kelvin, l.hs), (50.0, (2000.0, 6535.0), Some([30.5, 80.0])));
        assert_eq!((l.effects.len(), l.effect.as_deref()), (2, Some("candle")));
    }

    #[test]
    fn an_on_off_light_has_nothing_to_adjust() {
        let l = Light::of(&light(json!({ "supported_color_modes": ["onoff"] })));
        assert!(!l.brightness && !l.color_temp && !l.color && l.effects.is_empty());
    }

    #[test]
    fn a_call_sends_its_field_and_shows_it() {
        let c = ServiceCall::new("light", "turn_on", "light.x").state("on").field("brightness", json!(128), None);
        assert_eq!(c.data, json!({ "brightness": 128 }));
        assert_eq!(c.attributes, [("brightness".to_string(), json!(128))]);
    }
}
