//! The keyring dialog: a patina-shell overlay in the middle of the screen,
//! after polkit's. What it shows comes from the prompter as options; its
//! answer goes to stdout when it ends:
//!
//! - `yes`, then `choice 0|1` (the tick box, if there was one) and, for a
//!   password prompt, `password TEXT`;
//! - `no` when it's cancelled. Killed, it says nothing, which is the same.
//!
//! Three kinds: a password (one field), a new password (a field and a second
//! to repeat it in) and a question with just buttons. As on the lock screen
//! the password isn't kept in a text edit (its undo history would keep copies):
//! keys are read straight from egui's input, only dots are shown, and it's
//! wiped once sent.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::ExitCode;

use egui::{
    Align2, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use patina::skin::Skin;
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};
use zbus::zvariant::OwnedValue;

const WIDTH: u32 = 460;
/// The space between the dialog's edge and what's in it.
const MARGIN: egui::Vec2 = vec2(32.0, 26.0);
const BUTTON_HEIGHT: f32 = patina::dialog::BUTTON_HEIGHT;
const FIELD: egui::Vec2 = vec2(396.0, 44.0);
/// Longer than any password; what's typed past it is dropped.
const MAX_PASSWORD: usize = 1024;

/// What the dialog is for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Password,
    Confirm,
}

/// What the prompter asks for, as `--ask` options.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    kind: Kind,
    title: String,
    message: String,
    description: String,
    warning: String,
    /// A new password: asked for twice.
    new: bool,
    /// The tick box's words; none, no tick box.
    choice: Option<String>,
    chosen: bool,
    ok: String,
    cancel: String,
}

impl Request {
    /// From `PerformPrompt`'s type and properties.
    pub fn from_prompt(kind: &str, properties: &HashMap<String, OwnedValue>) -> Request {
        let text = |name: &str| {
            properties
                .get(name)
                .and_then(|v| v.downcast_ref::<&str>().ok())
                .unwrap_or_default()
                .to_string()
        };
        let flag = |name: &str| {
            properties
                .get(name)
                .and_then(|v| v.downcast_ref::<bool>().ok())
                .unwrap_or(false)
        };
        let choice = Some(text("choice-label")).filter(|s| !s.is_empty());
        Request {
            kind: if kind == "confirm" {
                Kind::Confirm
            } else {
                Kind::Password
            },
            title: text("title"),
            message: text("message"),
            description: text("description"),
            warning: text("warning"),
            new: flag("password-new"),
            choice,
            chosen: flag("choice-chosen"),
            ok: text("continue-label"),
            cancel: text("cancel-label"),
        }
    }

    pub fn has_choice(&self) -> bool {
        self.choice.is_some()
    }

    pub fn to_args(&self) -> Vec<String> {
        let mut args = vec![
            "--kind".into(),
            if self.kind == Kind::Confirm {
                "confirm"
            } else {
                "password"
            }
            .to_string(),
        ];
        for (flag, value) in [
            ("--title", &self.title),
            ("--message", &self.message),
            ("--description", &self.description),
            ("--warning", &self.warning),
            ("--ok", &self.ok),
            ("--cancel", &self.cancel),
        ] {
            args.push(flag.into());
            args.push(value.clone());
        }
        if self.new {
            args.push("--new".into());
        }
        if let Some(choice) = &self.choice {
            args.push("--choice".into());
            args.push(choice.clone());
        }
        if self.chosen {
            args.push("--chosen".into());
        }
        args
    }

    /// What `--ask` was given.
    pub fn parse(args: &[String]) -> Option<Request> {
        let mut r = Request {
            kind: Kind::Password,
            title: String::new(),
            message: String::new(),
            description: String::new(),
            warning: String::new(),
            new: false,
            choice: None,
            chosen: false,
            ok: String::new(),
            cancel: String::new(),
        };
        let mut args = args.iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--kind" => {
                    r.kind = match args.next()?.as_str() {
                        "confirm" => Kind::Confirm,
                        _ => Kind::Password,
                    }
                }
                "--title" => r.title = args.next()?.clone(),
                "--message" => r.message = args.next()?.clone(),
                "--description" => r.description = args.next()?.clone(),
                "--warning" => r.warning = args.next()?.clone(),
                "--ok" => r.ok = args.next()?.clone(),
                "--cancel" => r.cancel = args.next()?.clone(),
                "--choice" => r.choice = Some(args.next()?.clone()),
                "--new" => r.new = true,
                "--chosen" => r.chosen = true,
                _ => return None,
            }
        }
        Some(r)
    }

    fn fields(&self) -> usize {
        match (self.kind, self.new) {
            (Kind::Confirm, _) => 0,
            (Kind::Password, false) => 1,
            (Kind::Password, true) => 2,
        }
    }

    /// Tall enough for what's in it.
    fn size(&self) -> (u32, u32) {
        let lines = |s: &str, per: usize| s.chars().count().div_ceil(per).max(1) as u32;
        let mut h = 2 * MARGIN.y as u32 + BUTTON_HEIGHT as u32 + 24;
        h += 24 + 8; // title
        h += 19 * lines(&self.message, 52);
        if !self.description.is_empty() {
            h += 6 + 17 * lines(&self.description, 58);
        }
        if !self.warning.is_empty() {
            h += 6 + 17 * lines(&self.warning, 58);
        }
        h += self.fields() as u32 * 56;
        if self.fields() > 0 {
            h += 16;
        }
        if self.choice.is_some() {
            h += 34;
        }
        (WIDTH, h)
    }
}

struct Dialog {
    skin: Skin,
    request: Request,
    fields: Vec<String>,
    focus: usize,
    chosen: bool,
    /// Closed or answered.
    done: bool,
}

impl Dialog {
    fn new(skin: Skin, request: Request) -> Dialog {
        let fields = (0..request.fields())
            .map(|_| String::with_capacity(MAX_PASSWORD))
            .collect();
        let chosen = request.chosen;
        Dialog {
            skin,
            request,
            fields,
            focus: 0,
            chosen,
            done: false,
        }
    }

    /// Whether what's typed can be sent: something, and the same twice.
    fn ready(&self) -> bool {
        match self.fields.as_slice() {
            [] => true,
            [one] => !one.is_empty(),
            [a, b] => !a.is_empty() && a == b,
            _ => false,
        }
    }

    fn mismatch(&self) -> bool {
        matches!(self.fields.as_slice(), [a, b] if !b.is_empty() && !a.starts_with(b.as_str()))
    }

    fn answer(&mut self, yes: bool) {
        let mut out = std::io::stdout();
        if !yes {
            let _ = writeln!(out, "no").and_then(|()| out.flush());
            self.done = true;
            return;
        }
        let mut line = format!("yes\nchoice {}\n", u8::from(self.chosen));
        if self.request.new
            && let Some(password) = self.fields.first()
        {
            // The daemon asks "Store passwords unencrypted?" if this is 0.
            line.push_str(&format!("strength {}\n", strength(password)));
        }
        if let Some(password) = self.fields.first_mut() {
            line.push_str("password ");
            line.push_str(password);
            line.push('\n');
            wipe(password);
        }
        let _ = out.write_all(line.as_bytes()).and_then(|()| out.flush());
        let mut bytes = line.into_bytes();
        bytes.fill(0);
        std::hint::black_box(&bytes);
        self.done = true;
    }

    fn submit(&mut self) {
        if self.ready() {
            self.answer(true);
        }
    }

    fn typed(&mut self, ui: &Ui) {
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(text) => {
                    if let Some(field) = self.fields.get_mut(self.focus)
                        && field.len() + text.len() <= MAX_PASSWORD
                    {
                        field.push_str(&text);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => match key {
                    egui::Key::Enter if self.focus + 1 < self.fields.len() => self.focus += 1,
                    egui::Key::Tab if !self.fields.is_empty() => {
                        self.focus = (self.focus + 1) % self.fields.len();
                    }
                    egui::Key::Enter => self.submit(),
                    egui::Key::Backspace if modifiers.ctrl => self.clear(),
                    egui::Key::Backspace => {
                        if let Some(field) = self.fields.get_mut(self.focus) {
                            field.pop();
                        }
                    }
                    egui::Key::U if modifiers.ctrl => self.clear(),
                    egui::Key::Escape => self.answer(false),
                    _ => {}
                },
                _ => {}
            }
            if self.done {
                return;
            }
        }
    }

    fn clear(&mut self) {
        if let Some(field) = self.fields.get_mut(self.focus) {
            wipe(field);
        }
    }

    /// A pill with a dot for each character typed, and a caret in the one
    /// being typed in. Clicking one moves the typing to it.
    fn field(&mut self, ui: &mut Ui, index: usize, hint: &str) {
        let visuals = ui.visuals().clone();
        let time = ui.input(|i| i.time);
        let (rect, response) = ui.allocate_exact_size(FIELD, Sense::click());
        if response.clicked() {
            self.focus = index;
        }
        let focused = self.focus == index;
        let radius = CornerRadius::same((FIELD.y / 2.0) as u8);
        let painter = ui.painter();
        painter.rect_filled(rect, radius, visuals.extreme_bg_color);
        let ring = if focused {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.noninteractive.bg_stroke.color
        };
        painter.rect_stroke(rect, radius, Stroke::new(2.0, ring), StrokeKind::Inside);

        let left = rect.left() + 22.0;
        let step = 15.0;
        let fit = (((rect.width() - 44.0) / step) as usize).max(1);
        let count = self.fields[index].chars().count();
        let shown = count.min(fit);
        for i in 0..shown {
            painter.circle_filled(
                pos2(left + 4.0 + i as f32 * step, rect.center().y),
                4.5,
                visuals.text_color(),
            );
        }
        if count == 0 {
            painter.text(
                pos2(left + 6.0, rect.center().y),
                Align2::LEFT_CENTER,
                hint,
                FontId::proportional(16.0),
                visuals.weak_text_color(),
            );
        }
        if focused && (time * 2.0) as i64 % 2 == 0 {
            let x = left - 2.0 + shown as f32 * step;
            painter.line_segment(
                [pos2(x, rect.center().y - 10.0), pos2(x, rect.center().y + 10.0)],
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

        let inner = rect.shrink2(MARGIN);
        let mut text = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        let r = self.request.clone();
        let title = if r.title.is_empty() {
            "Keyring"
        } else {
            &r.title
        };
        text.label(RichText::new(title).size(19.0).strong());
        text.add_space(8.0);
        if !r.message.is_empty() {
            text.add(egui::Label::new(RichText::new(&r.message).size(14.0)).wrap());
        }
        if !r.description.is_empty() {
            text.add_space(6.0);
            text.add(
                egui::Label::new(
                    RichText::new(&r.description)
                        .size(13.0)
                        .color(visuals.weak_text_color()),
                )
                .wrap(),
            );
        }
        if !r.warning.is_empty() {
            text.add_space(6.0);
            text.add(
                egui::Label::new(
                    RichText::new(&r.warning)
                        .size(13.0)
                        .color(visuals.error_fg_color),
                )
                .wrap(),
            );
        }
        if !self.fields.is_empty() {
            text.add_space(16.0);
            let first = if r.new { "New password" } else { "Password" };
            self.field(&mut text, 0, first);
            if self.fields.len() == 2 {
                text.add_space(8.0);
                self.field(&mut text, 1, "Confirm password");
                text.add_space(4.0);
                if self.mismatch() {
                    text.label(
                        RichText::new("The passwords don't match.")
                            .size(13.0)
                            .color(visuals.error_fg_color),
                    );
                }
            }
        }
        if let Some(label) = &r.choice {
            text.add_space(10.0);
            text.checkbox(&mut self.chosen, label);
        }

        // The buttons, at the bottom right as a desktop's dialogs have them.
        let row = Rect::from_min_max(
            pos2(inner.left(), inner.bottom() - BUTTON_HEIGHT),
            inner.max,
        );
        let mut buttons = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        patina::dialog::buttons(&mut buttons);
        let ok = if r.ok.is_empty() {
            if r.new {
                "Create"
            } else if self.fields.is_empty() {
                "OK"
            } else {
                "Unlock"
            }
        } else {
            &r.ok
        };
        let ready = self.ready();
        let primary = if ready {
            patina::dialog::primary(&buttons, ok)
        } else {
            egui::Button::new(RichText::new(ok).color(visuals.weak_text_color()))
                .fill(visuals.widgets.inactive.weak_bg_fill)
        };
        if buttons.add_enabled(ready, primary).clicked() {
            self.submit();
        }
        let cancel = if r.cancel.is_empty() {
            "Cancel"
        } else {
            &r.cancel
        };
        if buttons.button(cancel).clicked() {
            self.answer(false);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// How good a new password is, 0 (none) to 4, as the daemon's
/// `password-strength`. Anything typed counts for at least 1: the daemon only
/// asks whether to go on without a password when this is 0.
fn strength(password: &str) -> i32 {
    if password.is_empty() {
        return 0;
    }
    let classes = [
        password.chars().any(char::is_lowercase),
        password.chars().any(char::is_uppercase),
        password.chars().any(|c| c.is_ascii_digit()),
        password.chars().any(|c| !c.is_alphanumeric()),
    ]
    .into_iter()
    .filter(|c| *c)
    .count();
    let len = password.chars().count();
    1 + i32::from(len >= 8) + i32::from(len >= 12) + i32::from(classes >= 3)
}

fn wipe(password: &mut String) {
    let mut bytes = std::mem::take(password).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
    password.reserve(MAX_PASSWORD);
}

/// Shows the dialog until it's answered.
pub fn run(request: Request) -> Result<ExitCode, String> {
    // The prompter holds our stdin open; when it closes, it has gone away
    // and there's nothing left to ask for.
    std::thread::Builder::new()
        .name("prompter-in".into())
        .spawn(|| {
            let mut sink = [0u8; 64];
            let mut stdin = std::io::stdin();
            while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
            std::process::exit(1);
        })
        .map_err(|e| e.to_string())?;

    let size = request.size();
    let dialog = Dialog::new(piece::skin("patina-keyring"), request);
    let options = Options {
        namespace: "patina-keyring".into(),
        layer: Layer::Overlay,
        anchor: Anchor::empty(),
        size,
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
    host.run(event_loop)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Request {
        Request {
            kind: Kind::Password,
            title: "Unlock Keyring".into(),
            message: "Enter password to unlock the login keyring".into(),
            description: "An application wants to access the keyring 'Login', but it is locked."
                .into(),
            warning: String::new(),
            new: false,
            choice: Some("Automatically unlock this keyring whenever I'm logged in".into()),
            chosen: false,
            ok: String::new(),
            cancel: String::new(),
        }
    }

    #[test]
    fn a_request_goes_through_its_options_and_back() {
        let mut r = request();
        r.warning = "The unlock password was incorrect".into();
        r.new = true;
        r.chosen = true;
        let args = r.to_args();
        assert_eq!(Request::parse(&args), Some(r));
        let q = Request {
            kind: Kind::Confirm,
            choice: None,
            ..request()
        };
        assert_eq!(Request::parse(&q.to_args()), Some(q));
        assert_eq!(Request::parse(&["--nonsense".into()]), None);
    }

    #[test]
    fn what_is_asked_comes_from_the_prompts_properties() {
        let owned = |v: zbus::zvariant::Value<'_>| OwnedValue::try_from(v).unwrap();
        let props = HashMap::from([
            ("title".to_string(), owned("Unlock".into())),
            ("password-new".to_string(), owned(true.into())),
            ("choice-label".to_string(), owned("Remember".into())),
            ("password-strength".to_string(), owned(3i32.into())),
        ]);
        let r = Request::from_prompt("password", &props);
        assert_eq!(r.title, "Unlock");
        assert!(r.new);
        assert_eq!(r.choice.as_deref(), Some("Remember"));
        assert_eq!(r.fields(), 2);
        assert_eq!(Request::from_prompt("confirm", &props).fields(), 0);
    }

    #[test]
    fn a_new_password_needs_the_same_twice() {
        let mut d = Dialog::new(
            Skin::default_skin(),
            Request {
                new: true,
                ..request()
            },
        );
        assert!(!d.ready());
        d.fields[0].push_str("hunter2");
        assert!(!d.ready());
        d.fields[1].push_str("hunter");
        assert!(!d.ready() && !d.mismatch());
        d.fields[1].push('3');
        assert!(d.mismatch());
        d.fields[1] = "hunter2".into();
        assert!(d.ready() && !d.mismatch());
    }

    #[test]
    fn a_typed_password_is_never_strength_zero() {
        assert_eq!(strength(""), 0);
        assert_eq!(strength("a"), 1);
        assert_eq!(strength("hunter2"), 1);
        assert!(strength("hunter2hunter") > strength("hunter2"));
        assert_eq!(strength("Correct-Horse-9-Battery"), 4);
    }

    #[test]
    fn a_question_needs_nothing_typed() {
        let d = Dialog::new(
            Skin::default_skin(),
            Request {
                kind: Kind::Confirm,
                ..request()
            },
        );
        assert!(d.ready());
    }

    /// Draws the dialog in each built-in skin as PNGs in the folder
    /// PATINA_KEYRING_PREVIEW names, so how it looks can be seen without a
    /// desktop: `PATINA_KEYRING_PREVIEW=/tmp/p cargo test -p patina-keyring
    /// preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_dialog() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_KEYRING_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        for skin in skins {
            let name = skin.name.clone();
            for state in ["unlock", "typed", "new", "confirm", "wrong"] {
                let mut r = request();
                match state {
                    "new" => {
                        r.new = true;
                        r.title = "Choose password for new keyring".into();
                        r.message = "Choose a password to protect the new keyring".into();
                    }
                    "confirm" => {
                        r.kind = Kind::Confirm;
                        r.choice = None;
                        r.message = "Store passwords unencrypted?".into();
                        r.description = "By choosing to use a blank password, your stored passwords will not be safely encrypted. They will be accessible by anyone with access to your files.".into();
                    }
                    "wrong" => r.warning = "The unlock password was incorrect".into(),
                    _ => {}
                }
                let (w, h) = r.size();
                let (w, h) = (w as usize, h as usize);
                let mut d = Dialog::new(skin.clone(), r);
                if state == "typed" {
                    d.fields[0] = "hunter2hunter".into();
                }
                if state == "new" {
                    d.fields[0] = "hunter2".into();
                    d.fields[1] = "hunter".into();
                    d.focus = 1;
                }
                let ctx = egui::Context::default();
                d.setup(&ctx);
                let mut painter = patina_shell::raster::Painter::default();
                let mut pixels = vec![0u32; w * h];
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            pos2(0.0, 0.0),
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
                let file = dir.join(format!("keyring-{name}-{state}.png"));
                image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
                    .unwrap();
            }
        }
    }
}
