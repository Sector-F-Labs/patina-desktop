//! Pieces shared by the entity list and the dashboards.

use crate::ha::{Control, Entity};

pub type Call = (String, String, String, Option<String>);

/// One entity: its name, state and control. Service calls are pushed to `calls`.
pub fn entity_row(ui: &mut egui::Ui, e: &Entity, name: Option<&str>, calls: &mut Vec<Call>) {
    ui.horizontal(|ui| {
        let name = egui::RichText::new(name.unwrap_or_else(|| e.name()));
        ui.label(if e.is_unavailable() { name.weak() } else { name });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| match e.control() {
            Control::Toggle(on) => {
                let mut value = on;
                if ui.add(egui::Checkbox::without_text(&mut value)).changed()
                    && let Some((domain, service, state)) = e.toggle_call(value)
                {
                    calls.push((domain.into(), service.into(), e.id.clone(), Some(state.into())));
                }
                ui.weak(e.display_state());
            }
            Control::Press => {
                if ui.button("Run").clicked()
                    && let Some((domain, service)) = e.press_call()
                {
                    calls.push((domain.into(), service.into(), e.id.clone(), None));
                }
            }
            Control::None => {
                ui.weak(e.display_state());
            }
        });
    });
}
