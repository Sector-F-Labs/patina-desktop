//! The Wi-Fi menu: a header with an on/off switch, then the networks in
//! range, best first. A click joins a saved or open network; a new secured
//! one asks for its password first; the one you're on offers Disconnect.

use std::sync::mpsc::Sender;

use egui::{
    Align, Color32, Layout, Rect, RichText, ScrollArea, Sense, StrokeKind, Ui, UiBuilder, pos2,
    vec2,
};
use patina::skin::Skin;
use patina_shell::Shell;
use patina_system::network::{Network, Security, Wifi};

const MARGIN: f32 = 12.0;
const ROW: f32 = 40.0;

/// What the menu asks of NetworkManager.
#[derive(Debug, Clone)]
pub enum Command {
    Connect(Network, Option<String>),
    Disconnect,
    SetEnabled(bool),
}

/// Opens `url` in the default browser, which the captive portal redirects to its
/// sign-in page.
fn open_browser(url: &str) {
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// A line under the header: progress, or what went wrong.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Busy(String),
    Problem(String),
}

pub struct WifiMenu {
    pub wifi: Option<Wifi>,
    pub status: Option<Status>,
    /// The network whose row is open: its password box, or Disconnect.
    open: Option<String>,
    password: String,
    had_focus: bool,
    done: bool,
    skin: Skin,
    commands: Sender<Command>,
}

impl WifiMenu {
    pub fn new(skin: Skin, commands: Sender<Command>) -> Self {
        Self {
            wifi: None,
            status: None,
            open: None,
            password: String::new(),
            had_focus: false,
            done: false,
            skin,
            commands,
        }
    }

    pub fn close(&mut self) {
        self.done = true;
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    fn connect(&mut self, network: &Network, password: Option<String>) {
        self.status = Some(Status::Busy(format!("Connecting to {}…", network.ssid)));
        self.send(Command::Connect(network.clone(), password));
        self.open = None;
        self.password.clear();
    }

    /// A click on `network`'s row.
    fn clicked(&mut self, network: &Network) {
        if self.open.as_deref() == Some(network.ssid.as_str()) {
            self.open = None;
            return;
        }
        let needs_password = matches!(network.security, Security::Psk | Security::Sae);
        if network.active || (needs_password && !network.saved) {
            self.open = Some(network.ssid.clone());
            self.password.clear();
        } else if network.security == Security::Other && !network.saved {
            self.status = Some(Status::Problem(format!(
                "{} needs a login this menu can't ask for; use nmtui",
                network.ssid
            )));
        } else {
            self.connect(network, None);
        }
    }

    fn header(&mut self, ui: &mut Ui, enabled: bool) {
        ui.horizontal(|ui| {
            ui.heading("Wi-Fi");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if switch(ui, enabled).clicked() {
                    self.send(Command::SetEnabled(!enabled));
                    if let Some(wifi) = self.wifi.as_mut() {
                        wifi.enabled = !enabled;
                    }
                }
            });
        });
    }

    fn row(&mut self, ui: &mut Ui, network: &Network) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        let v = ui.visuals().clone();
        let is_open = self.open.as_deref() == Some(network.ssid.as_str());
        if response.hovered() || is_open {
            ui.painter().rect_filled(
                rect,
                v.widgets.hovered.corner_radius,
                v.widgets.hovered.weak_bg_fill,
            );
        }
        let icon = Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(22.0, 22.0));
        patina_system::icons::wifi(ui.painter(), icon, Some(network.strength), v.text_color());
        let name_colour = if network.active {
            v.strong_text_color()
        } else {
            v.text_color()
        };
        let font = egui::TextStyle::Body.resolve(ui.style());
        let secured = network.security != Security::Open;
        let name = format!("{}{}", network.ssid, if secured { "  🔒" } else { "" });
        ui.painter().text(
            pos2(icon.right() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            font.clone(),
            name_colour,
        );
        let note = if network.active && self.wifi.as_ref().is_some_and(|w| w.sign_in.is_some()) {
            "Sign-in needed"
        } else if network.active {
            "Connected"
        } else if network.saved {
            "Saved"
        } else {
            ""
        };
        ui.painter().text(
            rect.right_center() - vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            note,
            egui::TextStyle::Small.resolve(ui.style()),
            v.weak_text_color(),
        );
        if response.clicked() {
            self.clicked(network);
        }
        if is_open {
            self.opened_row(ui, network);
        }
    }

    /// Under an open row: Disconnect for the active network, or a password
    /// box for a new secured one.
    fn opened_row(&mut self, ui: &mut Ui, network: &Network) {
        ui.horizontal(|ui| {
            ui.add_space(46.0);
            if network.active {
                if ui.button("Disconnect").clicked() {
                    self.status = None;
                    self.send(Command::Disconnect);
                    self.open = None;
                }
                return;
            }
            let field = ui.add(
                patina::dialog::text_edit(ui, &mut self.password)
                    .password(true)
                    .hint_text("Password")
                    .desired_width(ui.available_width() - 90.0),
            );
            field.request_focus();
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ready = !self.password.is_empty();
            if (ui
                .add_enabled(ready, egui::Button::new("Connect"))
                .clicked()
                || enter)
                && ready
            {
                let password = std::mem::take(&mut self.password);
                self.connect(network, Some(password));
            }
        });
        ui.add_space(4.0);
    }
}

/// An on/off switch, painted in the selection colour when on.
fn switch(ui: &mut Ui, on: bool) -> egui::Response {
    let size = vec2(36.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let v = ui.visuals();
    let track = if on {
        v.selection.bg_fill
    } else {
        v.widgets.inactive.bg_fill
    };
    ui.painter().rect_filled(rect, rect.height() / 2.0, track);
    let x = if on {
        rect.right() - rect.height() / 2.0
    } else {
        rect.left() + rect.height() / 2.0
    };
    ui.painter().circle_filled(
        pos2(x, rect.center().y),
        rect.height() / 2.0 - 3.0,
        Color32::WHITE,
    );
    response.on_hover_text(if on {
        "Turn Wi-Fi off"
    } else {
        "Turn Wi-Fi on"
    })
}

impl Shell for WifiMenu {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        if ui.input(|i| i.focused) {
            self.had_focus = true;
        } else if self.had_focus {
            self.done = true;
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            // Escape closes an open row first, then the menu.
            if self.open.take().is_none() {
                self.done = true;
                return;
            }
        }

        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        ui.painter().rect(
            rect,
            patina_shell::hud::corner_radius(),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink(MARGIN))
                .layout(Layout::top_down(Align::Min)),
        );

        let Some(wifi) = self.wifi.clone() else {
            ui.weak("Looking for Wi-Fi…");
            return;
        };
        if !wifi.present {
            ui.weak("No Wi-Fi device");
            return;
        }
        self.header(&mut ui, wifi.enabled);
        match &self.status {
            Some(Status::Busy(text)) => {
                ui.label(RichText::new(text).weak());
            }
            Some(Status::Problem(text)) => {
                ui.label(RichText::new(text).color(v.error_fg_color));
            }
            None => {}
        }
        if let Some(url) = &wifi.sign_in {
            ui.horizontal(|ui| {
                ui.label("This network needs you to sign in.");
                if ui.button("Sign in").clicked() {
                    open_browser(url);
                }
            });
        }
        ui.separator();
        if !wifi.enabled {
            ui.weak("Wi-Fi is off");
            return;
        }
        if wifi.networks.is_empty() {
            ui.weak("Looking for networks…");
            return;
        }
        ScrollArea::vertical()
            .auto_shrink(false)
            .show(&mut ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for network in &wifi.networks {
                    self.row(ui, network);
                }
            });
    }

    fn finished(&self) -> bool {
        self.done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patina_system::network::Security;
    use std::sync::mpsc;

    fn network(ssid: &str, security: Security, active: bool, saved: bool) -> Network {
        Network {
            ssid: ssid.into(),
            strength: 50,
            security,
            active,
            saved,
            access_point: "/ap".into(),
        }
    }

    fn menu() -> (WifiMenu, mpsc::Receiver<Command>) {
        let (send, receive) = mpsc::channel();
        (WifiMenu::new(Skin::default_skin(), send), receive)
    }

    #[test]
    fn saved_and_open_networks_join_on_a_click() {
        let (mut m, commands) = menu();
        m.clicked(&network("home", Security::Psk, false, true));
        assert!(matches!(commands.try_recv(), Ok(Command::Connect(n, None)) if n.ssid == "home"));
        m.clicked(&network("cafe", Security::Open, false, false));
        assert!(matches!(commands.try_recv(), Ok(Command::Connect(n, None)) if n.ssid == "cafe"));
        assert!(matches!(m.status, Some(Status::Busy(_))));
    }

    #[test]
    fn a_new_secured_network_asks_for_its_password_first() {
        let (mut m, commands) = menu();
        let n = network("office", Security::Psk, false, false);
        m.clicked(&n);
        assert_eq!(m.open.as_deref(), Some("office"));
        assert!(
            commands.try_recv().is_err(),
            "nothing sent before the password"
        );
        m.connect(&n, Some("hunter2".into()));
        assert!(matches!(commands.try_recv(), Ok(Command::Connect(_, Some(p))) if p == "hunter2"));
        assert_eq!(m.open, None);
    }

    #[test]
    fn the_active_network_opens_for_disconnect_and_enterprise_is_refused() {
        let (mut m, commands) = menu();
        m.clicked(&network("home", Security::Psk, true, true));
        assert_eq!(m.open.as_deref(), Some("home"));
        m.clicked(&network("eduroam", Security::Other, false, false));
        assert!(matches!(m.status, Some(Status::Problem(_))));
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn a_captive_portal_is_flagged_on_the_active_row() {
        let (mut m, _commands) = menu();
        assert!(m.wifi.is_none());
        m.wifi = Some(Wifi {
            sign_in: Some("http://check.example/".into()),
            ..Wifi::default()
        });
        assert!(m.wifi.as_ref().is_some_and(|w| w.sign_in.is_some()));
    }
}
