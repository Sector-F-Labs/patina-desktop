//! Draws a Lovelace dashboard natively (ADR-0004): the views of its config and the common cards.
//! A card type that isn't drawn shows its name instead, so a dashboard is never blank for want of one.

use std::collections::HashMap;

use serde_json::Value;

use crate::chart::{self, Chart};
use crate::ha::Entity;
use crate::history::Store;
use crate::palette::Palette;
use crate::widgets::{Event, entity_row};

/// Entities by id, and where what's clicked is collected.
pub struct Scene<'a> {
    pub states: HashMap<&'a str, &'a Entity>,
    pub events: &'a mut Vec<Event>,
    pub history: &'a Store,
    pub palette: &'a Palette,
    /// The (entity, hours) of history the cards drew, for the app to fetch.
    pub wanted: &'a mut Vec<(String, u32)>,
}

/// The widest a column of cards gets before another column is started.
const COLUMN_WIDTH: f32 = 360.0;

/// The titles of a config's views, for the tabs.
pub fn view_titles(config: &Value) -> Vec<String> {
    let views = config["views"].as_array().map(Vec::as_slice).unwrap_or_default();
    views
        .iter()
        .enumerate()
        .map(|(i, v)| {
            v["title"].as_str().or_else(|| v["path"].as_str()).map_or_else(|| format!("View {}", i + 1), str::to_string)
        })
        .collect()
}

/// Draws one view. Masonry and sidebar views flow their cards into columns, a sections view flows its
/// sections, and a panel view gives its first card the whole width.
pub fn show_view(ui: &mut egui::Ui, config: &Value, index: usize, scene: &mut Scene) {
    let Some(view) = config["views"].get(index) else {
        ui.weak("This dashboard has no views.");
        return;
    };
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match view["type"].as_str() {
        Some("panel") => {
            if let Some(card) = view["cards"].get(0) {
                card_ui(ui, card, scene);
            }
        }
        Some("sections") => {
            let sections: Vec<&Value> = view["sections"].as_array().into_iter().flatten().collect();
            columns(ui, &sections, |ui, section| {
                for card in section["cards"].as_array().into_iter().flatten() {
                    card_ui(ui, card, scene);
                }
            });
        }
        _ => {
            let cards: Vec<&Value> = view["cards"].as_array().into_iter().flatten().collect();
            if cards.is_empty() {
                ui.weak("This view has no cards.");
            }
            columns(ui, &cards, |ui, card| card_ui(ui, card, scene));
        }
    });
}

/// Deals `items` into as many columns as fit, left to right.
fn columns<'v>(ui: &mut egui::Ui, items: &[&'v Value], mut show: impl FnMut(&mut egui::Ui, &'v Value)) {
    let n = ((ui.available_width() / COLUMN_WIDTH) as usize).clamp(1, 4);
    ui.columns(n, |cols| {
        for (i, item) in items.iter().enumerate() {
            show(&mut cols[i % n], item);
        }
    });
}

/// A titled box, the shape of most cards.
fn framed(ui: &mut egui::Ui, title: Option<&str>, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(title) = title {
            ui.strong(title);
            ui.add_space(2.0);
        }
        add(ui);
    });
    ui.add_space(6.0);
}

/// What a card or row names: `"light.x"` or `{ entity: "light.x", name: "X" }`.
fn entity_ref(v: &Value) -> Option<(&str, Option<&str>)> {
    match v {
        Value::String(id) => Some((id, None)),
        Value::Object(_) => Some((v["entity"].as_str()?, v["name"].as_str())),
        _ => None,
    }
}

fn card_ui(ui: &mut egui::Ui, card: &Value, scene: &mut Scene) {
    let kind = card["type"].as_str().unwrap_or("");
    match kind {
        "heading" => {
            ui.add_space(4.0);
            ui.heading(card["heading"].as_str().or(card["title"].as_str()).unwrap_or(""));
        }
        "entities" => {
            framed(ui, card["title"].as_str(), |ui| {
                for row in card["entities"].as_array().into_iter().flatten() {
                    match entity_ref(row) {
                        Some((id, name)) => entity_line(ui, id, name, scene),
                        // Dividers, buttons and the like: not drawn.
                        None => continue,
                    }
                }
            });
        }
        "tile" | "light" | "button" | "entity" | "sensor" | "thermostat" | "gauge" => {
            let Some((id, name)) = entity_ref(card) else { return placeholder(ui, kind) };
            match scene.states.get(id).copied() {
                Some(e) if chart::is_graphable(e) => framed(ui, None, |ui| sensor_card(ui, e, name, scene)),
                _ => framed(ui, None, |ui| entity_line(ui, id, name, scene)),
            }
        }
        "glance" => framed(ui, card["title"].as_str(), |ui| {
            ui.horizontal_wrapped(|ui| {
                for item in card["entities"].as_array().into_iter().flatten() {
                    let Some((id, name)) = entity_ref(item) else { continue };
                    ui.vertical(|ui| {
                        ui.set_min_width(80.0);
                        match scene.states.get(id) {
                            Some(e) => {
                                ui.label(name.unwrap_or_else(|| e.name()));
                                ui.strong(e.display_state());
                            }
                            None => {
                                ui.weak(id);
                            }
                        }
                    });
                }
            });
        }),
        "markdown" => framed(ui, card["title"].as_str(), |ui| {
            // Plain text: the markup is shown as written.
            ui.label(card["content"].as_str().unwrap_or(""));
        }),
        "horizontal-stack" | "grid" => {
            let cards: Vec<&Value> = card["cards"].as_array().into_iter().flatten().collect();
            let n = if kind == "grid" { card["columns"].as_u64().unwrap_or(2) as usize } else { cards.len() };
            for row in cards.chunks(n.max(1)) {
                ui.columns(row.len(), |cols| {
                    for (col, card) in cols.iter_mut().zip(row) {
                        card_ui(col, card, scene);
                    }
                });
            }
        }
        "vertical-stack" => {
            for card in card["cards"].as_array().into_iter().flatten() {
                card_ui(ui, card, scene);
            }
        }
        _ => placeholder(ui, kind),
    }
}

/// A number with its unit, and its past day under it.
fn sensor_card(ui: &mut egui::Ui, e: &Entity, name: Option<&str>, scene: &mut Scene) {
    ui.horizontal(|ui| {
        let label = ui.add(egui::Label::new(name.unwrap_or_else(|| e.name())).sense(egui::Sense::click()));
        if label.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("History").clicked() {
            scene.events.push(Event::Open(e.id.clone()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.heading(e.display_state());
        });
    });
    scene.wanted.push((e.id.clone(), 24));
    let chart = Chart { entity: e, points: scene.history.get(&e.id, 24), hours: 24, palette: scene.palette };
    if chart.inline(ui, 64.0) {
        scene.events.push(Event::Open(e.id.clone()));
    }
}

fn placeholder(ui: &mut egui::Ui, kind: &str) {
    framed(ui, None, |ui| {
        ui.weak(format!("Card type “{kind}” isn't supported yet"));
    });
}

fn entity_line(ui: &mut egui::Ui, id: &str, name: Option<&str>, scene: &mut Scene) {
    match scene.states.get(id) {
        Some(e) => entity_row(ui, e, name, scene.events),
        None => {
            ui.weak(format!("{id} isn't available"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn entity_refs_are_ids_or_objects() {
        assert_eq!(entity_ref(&json!("light.x")), Some(("light.x", None)));
        assert_eq!(entity_ref(&json!({ "entity": "light.x", "name": "X" })), Some(("light.x", Some("X"))));
        assert_eq!(entity_ref(&json!({ "type": "divider" })), None);
    }

    #[test]
    fn views_are_named_by_title_then_path() {
        let config = json!({ "views": [{ "title": "Home" }, { "path": "rooms" }, {}] });
        assert_eq!(view_titles(&config), ["Home", "rooms", "View 3"]);
    }

    /// Runs `show_view` in a headless egui frame; returns what it drew without panicking.
    #[test]
    fn every_demo_view_draws() {
        let entities = crate::ha::demo();
        let dashboards = crate::ha::demo_dashboards();
        let config = dashboards[0].config.as_ref().unwrap();
        let ctx = egui::Context::default();
        let (history, palette) = (Store::default(), Palette::new());
        for index in 0..view_titles(config).len() {
            let mut events = Vec::new();
            let mut wanted = Vec::new();
            let _ = ctx.run_ui(Default::default(), |ui| {
                let mut scene = Scene {
                    states: entities.iter().map(|e| (e.id.as_str(), e)).collect(),
                    events: &mut events,
                    history: &history,
                    palette: &palette,
                    wanted: &mut wanted,
                };
                show_view(ui, config, index, &mut scene);
            });
        }
    }
}
