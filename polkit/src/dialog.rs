//! The password dialog (ADR-0128): a patina-shell overlay in the middle of
//! the screen with what the app is asking for, who's authenticating, and a
//! box for the password. It's run by the agent as `--ask` and talks to it on
//! stdin and stdout, a line at a time:
//!
//! - from the agent: `prompt TEXT`, `info TEXT`, `wrong`, `ok`;
//! - to the agent: `password TEXT`. Closing the dialog is how it cancels.
//!
//! As on the lock screen the password isn't kept in a text edit (its undo
//! history would keep copies): keys are read straight from egui's input, only
//! dots are shown, and it's wiped once sent.

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use egui::{Align2, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use patina::skin::Skin;
use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

const SIZE: (u32, u32) = (460, 262);
/// The space between the dialog's edge and what's in it.
const MARGIN: egui::Vec2 = vec2(32.0, 26.0);
/// How high the buttons are: patina-ui's bar buttons (a title bar's or tool
/// bar's), 24, rather than egui's own.
const BUTTON_HEIGHT: f32 = 24.0;
const FIELD: egui::Vec2 = vec2(396.0, 44.0);
const SHAKE: f32 = 0.45;
/// Longer than any password; what's typed past it is dropped.
const MAX_PASSWORD: usize = 1024;

/// What `--ask` was given.
pub struct Request {
    user: String,
    message: String,
}

impl Request {
    pub fn parse(args: &[String]) -> Option<Request> {
        let mut user = None;
        let mut message = None;
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--user" => user = Some(args.next()?.clone()),
                "--message" => message = Some(args.next()?.clone()),
                _ => return None,
            }
        }
        Some(Request {
            user: user?,
            message: message.unwrap_or_default(),
        })
    }
}

/// What the agent says.
enum Said {
    Prompt(String),
    Info(String),
    Wrong,
    Ok,
}

fn parse(line: &str) -> Option<Said> {
    let (word, text) = line.split_once(' ').unwrap_or((line, ""));
    Some(match word {
        "prompt" => Said::Prompt(text.trim().trim_end_matches(':').trim().to_string()),
        "info" => Said::Info(text.trim().to_string()),
        "wrong" => Said::Wrong,
        "ok" => Said::Ok,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq)]
enum Status {
    Typing,
    Checking,
    Wrong,
}

struct Dialog {
    skin: Skin,
    user: String,
    message: String,
    /// What the field asks for, in the PAM module's words.
    prompt: String,
    info: String,
    password: String,
    status: Status,
    wrong_at: Option<Instant>,
    /// Closed, by Cancel or Escape.
    cancelled: bool,
    /// Authenticated.
    done: bool,
    send: mpsc::Sender<String>,
}

impl Dialog {
    fn said(&mut self, said: Said) {
        match said {
            Said::Prompt(p) => {
                self.prompt = p;
                self.status = Status::Typing;
            }
            Said::Info(i) => self.info = i,
            Said::Wrong => {
                self.status = Status::Wrong;
                self.wrong_at = Some(Instant::now());
            }
            Said::Ok => self.done = true,
        }
    }

    fn submit(&mut self) {
        if self.password.is_empty() || self.status == Status::Checking {
            return;
        }
        let mut password = std::mem::take(&mut self.password);
        let sent = self.send.send(format!("password {password}"));
        wipe(&mut password);
        if sent.is_ok() {
            self.status = Status::Checking;
        }
    }

    fn typed(&mut self, ui: &Ui) {
        if self.status == Status::Checking {
            // Escape still closes it.
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.cancelled = true;
            }
            return;
        }
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(text) => {
                    if self.password.len() + text.len() <= MAX_PASSWORD {
                        self.password.push_str(&text);
                    }
                    self.status = Status::Typing;
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    egui::Key::Enter => self.submit(),
                    egui::Key::Backspace if modifiers.ctrl => wipe(&mut self.password),
                    egui::Key::Backspace => {
                        self.password.pop();
                    }
                    egui::Key::U if modifiers.ctrl => wipe(&mut self.password),
                    egui::Key::Escape => self.cancelled = true,
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// The pill the password goes in: a dot for each character and a caret,
    /// a ring in the error colour and a shake when it's refused.
    fn password_box(&self, ui: &mut Ui) {
        let visuals = ui.visuals().clone();
        let time = ui.input(|i| i.time);
        let (rect, _) = ui.allocate_exact_size(FIELD, Sense::hover());
        let shake = self
            .wrong_at
            .map(|t| t.elapsed().as_secs_f32())
            .filter(|s| *s < SHAKE)
            .map_or(0.0, |s| (s * 45.0).sin() * 10.0 * (1.0 - s / SHAKE));
        let rect = rect.translate(vec2(shake, 0.0));
        let radius = CornerRadius::same((FIELD.y / 2.0) as u8);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, visuals.extreme_bg_color);
        let ring = if self.status == Status::Wrong {
            visuals.error_fg_color
        } else {
            visuals.selection.bg_fill
        };
        painter.rect_stroke(rect, radius, Stroke::new(2.0, ring), StrokeKind::Inside);

        let checking = self.status == Status::Checking;
        let left = rect.left() + 22.0;
        let step = 15.0;
        let fit = (((rect.width() - 44.0) / step) as usize).max(1);
        let count = self.password.chars().count();
        let shown = count.min(fit);
        let dim = if checking { 0.45 } else { 1.0 };
        for i in 0..shown {
            painter.circle_filled(
                egui::pos2(left + 4.0 + i as f32 * step, rect.center().y),
                4.5,
                visuals.text_color().gamma_multiply(dim),
            );
        }
        if count == 0 {
            let hint = if self.prompt.is_empty() {
                "Password"
            } else {
                &self.prompt
            };
            painter.text(
                egui::pos2(left + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                hint,
                FontId::proportional(16.0),
                visuals.weak_text_color(),
            );
        }
        if !checking && (time * 2.0) as i64 % 2 == 0 {
            let x = left - 2.0 + shown as f32 * step;
            painter.line_segment(
                [
                    egui::pos2(x, rect.center().y - 10.0),
                    egui::pos2(x, rect.center().y + 10.0),
                ],
                Stroke::new(1.5, visuals.text_color()),
            );
        }
    }
}

impl Shell for Dialog {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _output: &str) {
        self.typed(ui);
        let rect = ui.max_rect();
        let visuals = ui.visuals().clone();
        let radius = CornerRadius::same(12);
        ui.painter().rect_filled(rect, radius, visuals.panel_fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            StrokeKind::Inside,
        );

        // The text and the box, centred inside the margins.
        let inner = rect.shrink2(vec2(MARGIN.x, MARGIN.y));
        let mut text = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        text.label(RichText::new("Authentication Required").size(19.0).strong());
        text.add_space(8.0);
        text.add(egui::Label::new(RichText::new(&self.message).size(14.0)).wrap());
        text.add_space(2.0);
        text.label(
            RichText::new(format!("as {}", self.user))
                .size(13.0)
                .color(visuals.weak_text_color()),
        );
        text.add_space(16.0);
        self.password_box(&mut text);
        text.add_space(8.0);
        let (note, colour) = match &self.status {
            Status::Wrong => ("Sorry, that didn't work. Try again.", visuals.error_fg_color),
            Status::Checking => ("Checking…", visuals.weak_text_color()),
            Status::Typing => (self.info.as_str(), visuals.weak_text_color()),
        };
        text.label(RichText::new(note).size(13.0).color(colour));

        // The buttons, at the bottom right as a desktop's dialogs have them,
        // the size of the ones in a window's title bar: as wide as their text
        // and 10 of padding, 24 high (patina-ui's bar buttons), not egui's.
        let row = Rect::from_min_max(pos2(inner.left(), inner.bottom() - BUTTON_HEIGHT), inner.max);
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        buttons.spacing_mut().button_padding = vec2(10.0, 5.0);
        buttons.spacing_mut().interact_size.y = BUTTON_HEIGHT;
        buttons.spacing_mut().item_spacing.x = 8.0;
        let ready = !self.password.is_empty() && self.status != Status::Checking;
        let label = RichText::new("Authenticate").color(if ready {
            visuals.selection.stroke.color
        } else {
            visuals.weak_text_color()
        });
        let primary = egui::Button::new(label).fill(if ready {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.inactive.weak_bg_fill
        });
        if buttons.add_enabled(ready, primary).clicked() {
            self.submit();
        }
        if buttons.button("Cancel").clicked() {
            self.cancelled = true;
        }

        let busy = self.status == Status::Checking
            || self
                .wrong_at
                .is_some_and(|t| t.elapsed().as_secs_f32() < SHAKE);
        ui.ctx().request_repaint_after(Duration::from_millis(if busy {
            16
        } else {
            500
        }));
    }

    fn finished(&self) -> bool {
        self.cancelled || self.done
    }
}

fn wipe(password: &mut String) {
    let mut bytes = std::mem::take(password).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
    password.reserve(MAX_PASSWORD);
}

/// Shows the dialog until the password is accepted (exit 0) or it's closed
/// (exit 1).
pub fn run(request: Request) -> Result<ExitCode, String> {
    // The agent's words arrive on a thread of their own, the password goes
    // out on another: stdin and stdout block.
    let (said, heard) = channel::channel::<Said>();
    std::thread::Builder::new()
        .name("agent-in".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines().map_while(Result::ok) {
                if let Some(s) = parse(&line)
                    && said.send(s).is_err()
                {
                    return;
                }
            }
            // The agent went away: nothing left to ask for.
            std::process::exit(1);
        })
        .map_err(|e| e.to_string())?;
    let (send, outgoing) = mpsc::channel::<String>();
    std::thread::Builder::new()
        .name("agent-out".into())
        .spawn(move || {
            let mut stdout = std::io::stdout();
            while let Ok(mut line) = outgoing.recv() {
                let written = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
                // SAFETY of the wipe is as the others': zero, then drop.
                let mut bytes = std::mem::take(&mut line).into_bytes();
                bytes.fill(0);
                std::hint::black_box(&bytes);
                if written.is_err() {
                    return;
                }
            }
        })
        .map_err(|e| e.to_string())?;

    let dialog = Dialog {
        skin: piece::skin("patina-polkit"),
        user: request.user,
        message: request.message,
        prompt: String::new(),
        info: String::new(),
        password: String::with_capacity(MAX_PASSWORD),
        status: Status::Typing,
        wrong_at: None,
        cancelled: false,
        done: false,
        send,
    };
    let options = Options {
        namespace: "patina-polkit".into(),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        size: SIZE,
        exclusive_zone: 0,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::Exclusive,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: true,
    };
    let (host, event_loop) = Host::connect(options, dialog)?;
    event_loop
        .handle()
        .insert_source(heard, |event, _, host| {
            if let Event::Msg(said) = event {
                host.shell.said(said);
                host.redraw_all();
            }
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(skin: Skin) -> Dialog {
        let (send, _sent) = mpsc::channel();
        Dialog {
            skin,
            user: "divan".into(),
            message: "Authentication is required to run the GParted Partition Editor as root".into(),
            prompt: "Password".into(),
            info: String::new(),
            password: String::new(),
            status: Status::Typing,
            wrong_at: None,
            cancelled: false,
            done: false,
            send,
        }
    }

    #[test]
    fn the_agents_words_are_understood() {
        assert!(matches!(parse("prompt Password: "), Some(Said::Prompt(p)) if p == "Password"));
        assert!(matches!(parse("wrong "), Some(Said::Wrong)));
        assert!(matches!(parse("ok "), Some(Said::Ok)));
        assert!(parse("nonsense").is_none());
    }

    #[test]
    fn a_password_is_sent_once_and_wiped() {
        let (send, sent) = mpsc::channel();
        let mut d = dialog(Skin::default_skin());
        d.send = send;
        d.password.push_str("hunter2");
        d.submit();
        assert_eq!(sent.try_recv().as_deref(), Ok("password hunter2"));
        assert!(d.password.is_empty());
        assert_eq!(d.status, Status::Checking);
        // Not again while it's being checked.
        d.password.push('x');
        d.submit();
        assert!(sent.try_recv().is_err());
    }

    /// Draws the dialog in each built-in skin, empty, typed into, refused and
    /// checking, as PNGs in the folder PATINA_POLKIT_PREVIEW names, so how it
    /// looks can be seen without a desktop: `PATINA_POLKIT_PREVIEW=/tmp/p
    /// cargo test -p patina-polkit preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_dialog() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_POLKIT_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let (w, h) = (SIZE.0 as usize, SIZE.1 as usize);
        for skin in skins {
            let name = skin.name.clone();
            for state in ["empty", "typed", "wrong", "checking"] {
                let mut d = dialog(skin.clone());
                match state {
                    "typed" => d.password = "hunter2hunter".into(),
                    "wrong" => {
                        d.said(Said::Wrong);
                        d.wrong_at = None;
                    }
                    "checking" => {
                        d.password = "hunter2".into();
                        d.submit();
                    }
                    _ => {}
                }
                let ctx = egui::Context::default();
                d.setup(&ctx);
                let mut painter = patina_shell::raster::Painter::default();
                let mut pixels = vec![0u32; w * h];
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::pos2(0.0, 0.0),
                            vec2(w as f32, h as f32),
                        )),
                        max_texture_side: Some(8192),
                        ..Default::default()
                    };
                    let out = ctx.run_ui(input, |ui| d.ui(ui, ""));
                    let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                    painter.paint(
                        &mut pixels,
                        w,
                        h,
                        out.pixels_per_point,
                        &primitives,
                        &out.textures_delta,
                    );
                }
                let rgba: Vec<u8> = pixels
                    .iter()
                    .flat_map(|p| {
                        let [b, g, r, a] = p.to_le_bytes();
                        [r, g, b, a]
                    })
                    .collect();
                let file = dir.join(format!("polkit-{name}-{state}.png"));
                image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .unwrap();
            }
        }
    }
}
