//! The Game mode page (ADR-0134): which programs to quit and which commands to
//! run while a fullscreen window has the keyboard, written into
//! patina-game-mode's own config (`~/.config/patina-game-mode/config.toml`), as
//! Dictation's is. Whether game mode is on comes from the marker file it keeps
//! in `XDG_RUNTIME_DIR`, and the button to switch it by hand leaves a request
//! file it takes.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use egui::{RichText, Ui};
use toml_edit::{DocumentMut, value};

use crate::app::Message;

/// patina-game-mode's config.
fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("patina-game-mode/config.toml"))
}

/// The longest delay it takes, in seconds.
pub const MOST_DELAY: u64 = 300;

/// What patina-game-mode's config holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMode {
    pub enabled: bool,
    /// Seconds a fullscreen window must hold the keyboard.
    pub delay: u64,
    pub kill: Vec<String>,
    pub on_enter: Vec<String>,
    pub on_exit: Vec<String>,
}

impl Default for GameMode {
    fn default() -> Self {
        Self {
            enabled: false,
            delay: 3,
            kill: Vec::new(),
            on_enter: Vec::new(),
            on_exit: Vec::new(),
        }
    }
}

/// The settings from the config's text, the defaults for what's missing or wrong.
pub fn parse(text: &str) -> GameMode {
    let mut game = GameMode::default();
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return game;
    };
    let list = |key: &str| -> Vec<String> {
        doc.get(key)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    if let Some(on) = doc.get("enabled").and_then(|v| v.as_bool()) {
        game.enabled = on;
    }
    if let Some(d) = doc.get("delay").and_then(|v| v.as_integer())
        && (0..=MOST_DELAY as i64).contains(&d)
    {
        game.delay = d as u64;
    }
    game.kill = list("kill");
    game.on_enter = list("on_enter");
    game.on_exit = list("on_exit");
    game
}

/// `text` with the settings set to `game`, everything else (comments too) kept.
pub fn with_settings(text: &str, game: &GameMode) -> Result<String, String> {
    let mut doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
    let list = |items: &[String]| items.iter().collect::<toml_edit::Array>();
    doc["enabled"] = value(game.enabled);
    doc["delay"] = value(game.delay as i64);
    doc["kill"] = value(list(&game.kill));
    doc["on_enter"] = value(list(&game.on_enter));
    doc["on_exit"] = value(list(&game.on_exit));
    Ok(doc.to_string())
}

fn read() -> GameMode {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| parse(&t))
        .unwrap_or_default()
}

/// Writes the settings into the config, making it if need be.
fn write(game: &GameMode) -> Result<(), String> {
    let path = config_path().ok_or("there's no config folder")?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let text = with_settings(&text, game)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Beside it and renamed over it, so nothing reads half a file.
    let partial = path.with_extension("toml.partial");
    std::fs::write(&partial, text)
        .and_then(|()| std::fs::rename(&partial, &path))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// How often the marker file is looked for.
const POLL: Duration = Duration::from_secs(1);

fn runtime_file(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(dir).join("patina").join(name))
}

pub struct GameModePage {
    settings: GameMode,
    /// What each list's add field holds, by list.
    typing: [String; 3],
    on: bool,
    updates: mpsc::Receiver<bool>,
}

impl GameModePage {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            settings: read(),
            typing: Default::default(),
            on: false,
            updates: watch(ctx.clone()),
        }
    }

    fn save(&mut self, message: &mut Option<Message>) {
        if let Err(e) = write(&self.settings) {
            *message = Some(Message::error(format!("Couldn't save Game mode: {e}")));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        while let Ok(on) = self.updates.try_recv() {
            self.on = on;
        }
        let before = self.settings.clone();

        ui.heading("Game mode");
        ui.label(
            "Frees the machine's resources while you play. Patina notices when a window goes \
             fullscreen and has the keyboard; once it has held it for a few seconds, it quits \
             the programs below and runs your commands, and when that window leaves or loses \
             the keyboard, it runs the commands for the end. Nothing is put back except by those.",
        );
        ui.add_space(8.0);
        self.state(ui, message);
        ui.add_space(8.0);

        ui.checkbox(
            &mut self.settings.enabled,
            "Turn on automatically when a window goes fullscreen",
        );
        ui.label(
            RichText::new(
                "A fullscreen film or presentation counts too. Nothing below happens while \
                 this is off, except when you switch game mode on by hand. Needs Night Light \
                 on, with \"pause in fullscreen\".",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            ui.label("Wait");
            ui.add(
                egui::DragValue::new(&mut self.settings.delay)
                    .range(0..=MOST_DELAY)
                    .suffix(" s"),
            );
            ui.label("before it counts, and before it ends.");
        });
        ui.label(
            RichText::new(
                "So that switching away from the game and back doesn't run everything each time.",
            )
            .small()
            .weak(),
        );

        section(ui, "Programs to quit");
        ui.label(
            RichText::new(
                "Name a program as it appears in the process list. Each is told to quit when \
                 game mode starts and isn't started again, so whatever it hadn't saved is lost. \
                 Only your own programs are touched, and a name matches only the first 15 \
                 characters the system keeps, so `node` matches every Node program.",
            )
            .small()
            .weak(),
        );
        list(
            ui,
            "kill",
            &mut self.settings.kill,
            &mut self.typing[0],
            "rust-analyzer",
        );

        section(ui, "Commands");
        ui.label(
            RichText::new(
                "Each runs in your session through sh, one after another, and is stopped if it \
                 takes over 30 seconds. For example `docker stop $(docker ps -q)` on starting, \
                 or `systemctl --user stop syncthing` on starting and `systemctl --user start \
                 syncthing` on ending.",
            )
            .small()
            .weak(),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("When game mode starts").strong());
        list(
            ui,
            "enter",
            &mut self.settings.on_enter,
            &mut self.typing[1],
            "docker stop $(docker ps -q)",
        );
        ui.add_space(6.0);
        ui.label(RichText::new("When it ends").strong());
        list(
            ui,
            "exit",
            &mut self.settings.on_exit,
            &mut self.typing[2],
            "systemctl --user start syncthing",
        );

        ui.add_space(12.0);
        ui.label(
            RichText::new(
                "Ctrl+Super+G switches game mode on or off by hand, whatever is on screen; \
                 pressed again, it goes back to following the fullscreen window. (If you've \
                 written your own [bindings] in the compositor's config, add \
                 \"Ctrl+Super+G\" = \"patina-game-mode --toggle\" to it.)",
            )
            .small()
            .weak(),
        );

        if self.settings != before {
            self.save(message);
        }
    }

    /// Whether game mode is on, and the button to switch it.
    fn state(&mut self, ui: &mut Ui, message: &mut Option<Message>) {
        ui.horizontal(|ui| {
            ui.label(if self.on { "On now." } else { "Off now." });
            let label = if self.on {
                "Turn off now"
            } else {
                "Turn on now"
            };
            if ui.button(label).clicked() {
                let made = runtime_file("game-mode.toggle")
                    .ok_or("no runtime folder".to_string())
                    .and_then(|p| {
                        if let Some(dir) = p.parent() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                        std::fs::write(&p, "").map_err(|e| e.to_string())
                    });
                match made {
                    Ok(()) => {}
                    Err(e) => {
                        *message = Some(Message::error(format!("Couldn't reach game mode: {e}")))
                    }
                }
            }
        });
        if !self.on {
            ui.label(
                RichText::new(
                    "Needs patina-game-mode running; the compositor starts it with the session.",
                )
                .small()
                .weak(),
            );
        }
    }
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(14.0);
    ui.separator();
    ui.add_space(4.0);
    ui.label(RichText::new(title).heading().size(16.0));
}

/// A list of text items, each with a button to remove it, and a field to add one.
fn list(ui: &mut Ui, id: &str, items: &mut Vec<String>, typing: &mut String, hint: &str) {
    ui.push_id(id, |ui| {
        let mut remove = None;
        for (i, item) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("✕").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
                ui.monospace(item);
            });
        }
        if let Some(i) = remove {
            items.remove(i);
        }
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(typing)
                    .hint_text(hint)
                    .desired_width(260.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let text = typing.trim().to_string();
            let add = ui
                .add_enabled(!text.is_empty(), egui::Button::new("Add"))
                .clicked()
                || (enter && !text.is_empty());
            if add && !items.contains(&text) {
                items.push(text);
                typing.clear();
            } else if add {
                typing.clear();
            }
        });
    });
}

/// Looks for the marker file every second, on a thread, sending when it changes.
fn watch(ctx: egui::Context) -> mpsc::Receiver<bool> {
    let (send, receive) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("game-mode".into())
        .spawn(move || {
            let mut last = None;
            loop {
                let now = runtime_file("game-mode").is_some_and(|p| p.exists());
                if last != Some(now) {
                    if send.send(now).is_err() {
                        return;
                    }
                    ctx.request_repaint();
                    last = Some(now);
                }
                std::thread::sleep(POLL);
            }
        });
    receive
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_round_trips_and_keeps_the_rest() {
        let game = GameMode {
            enabled: true,
            delay: 8,
            kill: vec!["rust-analyzer".into(), "node".into()],
            on_enter: vec!["docker stop $(docker ps -q)".into()],
            on_exit: vec!["echo out".into()],
        };
        let text = with_settings("# my notes\nextra = 1\n", &game).unwrap();
        assert!(
            text.contains("# my notes") && text.contains("extra = 1"),
            "{text}"
        );
        assert_eq!(parse(&text), game);
        let text = with_settings(&text, &GameMode::default()).unwrap();
        assert_eq!(parse(&text), GameMode::default());
    }

    #[test]
    fn a_broken_config_is_the_defaults() {
        assert_eq!(
            parse("delay = 9999\nkill = \"nope\"\n"),
            GameMode::default()
        );
        assert_eq!(parse("garbage ="), GameMode::default());
    }
}
