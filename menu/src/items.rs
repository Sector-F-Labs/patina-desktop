//! A menu of any items (ADR-0056), for a piece too small to hold one: the
//! bar's window tabs open theirs this way, so it isn't cut off at the bar's
//! edge. The caller names the items; the one chosen is printed, and the
//! menu closes.

use egui::Ui;
use patina::skin::Skin;
use patina_shell::Shell;

use crate::overlay::{self, Place};

/// The narrowest a menu of items is.
const WIDTH: f32 = 180.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// What's printed when it's chosen, and what it says.
    Choice {
        id: String,
        label: String,
    },
    Separator,
}

/// `ID=LABEL`, as `--item` takes it; the label is the ID without one.
pub fn choice(text: &str) -> Option<Item> {
    let (id, label) = text.split_once('=').unwrap_or((text, text));
    (!id.is_empty()).then(|| Item::Choice {
        id: id.to_string(),
        label: label.to_string(),
    })
}

pub struct ItemMenu {
    place: Place,
    items: Vec<Item>,
    skin: Skin,
    had_focus: bool,
    /// The item chosen, once it is.
    pub chosen: Option<String>,
    done: bool,
}

impl ItemMenu {
    pub fn new(place: Place, items: Vec<Item>) -> Self {
        Self {
            place,
            items,
            skin: crate::bar_skin(),
            had_focus: false,
            chosen: None,
            done: false,
        }
    }

    fn items(&mut self, ui: &mut Ui) {
        ui.set_min_width(WIDTH);
        for item in &self.items {
            match item {
                Item::Choice { id, label } => {
                    if ui.button(label).clicked() {
                        self.chosen = Some(id.clone());
                    }
                }
                Item::Separator => {
                    ui.separator();
                }
            }
        }
    }
}

impl Shell for ItemMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        patina::appearance::watch(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        if overlay::closing(ui, &mut self.had_focus) {
            self.done = true;
            return;
        }
        let place = self.place;
        overlay::show(ui, "items", place, |ui| self.items(ui));
        if let Some(id) = &self.chosen {
            println!("{id}");
            self.done = true;
        } else if overlay::pressed_outside(ui) {
            self.done = true;
        }
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::Edge;
    use egui::{Pos2, pos2};

    fn frame(menu: &mut ItemMenu, ctx: &egui::Context, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            focused: true,
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| menu.ui(ui, ""));
    }

    fn click(at: Pos2) -> [Vec<egui::Event>; 2] {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        [
            vec![egui::Event::PointerMoved(at), button(true)],
            vec![button(false)],
        ]
    }

    fn menu(edge: Edge) -> ItemMenu {
        let items = ["new=New Window", "close=Close Window"]
            .into_iter()
            .filter_map(choice)
            .collect();
        ItemMenu::new(
            Place {
                at: pos2(300.0, 50.0),
                edge,
            },
            items,
        )
    }

    #[test]
    fn items_read_as_id_and_label() {
        assert_eq!(
            choice("new=New Window"),
            Some(Item::Choice {
                id: "new".into(),
                label: "New Window".into()
            })
        );
        assert_eq!(
            choice("close"),
            Some(Item::Choice {
                id: "close".into(),
                label: "close".into()
            })
        );
        assert_eq!(choice("=x"), None);
    }

    #[test]
    fn from_the_bottom_it_opens_upward_and_a_click_chooses() {
        let ctx = egui::Context::default();
        let mut m = menu(Edge::Bottom);
        frame(&mut m, &ctx, Vec::new());
        frame(&mut m, &ctx, Vec::new());
        let area = ctx
            .memory(|mem| mem.area_rect(egui::Id::new("items")))
            .unwrap();
        assert_eq!(area.bottom(), 950.0, "50 up from the bottom");
        assert_eq!(area.left(), 300.0);
        // The first item, near the menu's top.
        let first = pos2(area.left() + 20.0, area.top() + 14.0);
        for events in click(first) {
            frame(&mut m, &ctx, events);
        }
        assert_eq!(m.chosen.as_deref(), Some("new"));
        assert!(m.done);
    }

    #[test]
    fn a_click_elsewhere_closes_it_with_nothing_chosen() {
        let ctx = egui::Context::default();
        let mut m = menu(Edge::Top);
        frame(&mut m, &ctx, Vec::new());
        for events in click(pos2(1200.0, 800.0)) {
            frame(&mut m, &ctx, events);
        }
        assert!(m.done);
        assert_eq!(m.chosen, None);
    }
}
