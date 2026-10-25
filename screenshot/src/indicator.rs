//! The pill shown while the desktop records the screen (ADR-0092): a red
//! dot that breathes, the time recorded, and a Stop button. The compositor
//! starts it with the recording and closes its stdin when it ends; Stop
//! prints `stop` and exits, and the compositor ends the recording. It takes
//! no keyboard, and it's left out of the recording.

use std::cell::Cell;
use std::io::Read;
use std::rc::Rc;

use egui::{Align2, CornerRadius, Rect, Sense, StrokeKind, TextStyle, Ui, pos2, vec2};
use patina::skin::Skin;
use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

/// The pill, in logical pixels.
pub const SIZE: (u32, u32) = (210, 44);
const GAP: i32 = 24;
const FADE: f64 = 0.15;

pub struct Indicator {
    skin: Skin,
    started: Option<f64>,
    ended_at: Option<f64>,
    ending: bool,
    /// Set when Stop is pressed, for `run` to read once it's gone.
    stopped: Rc<Cell<bool>>,
    done: bool,
}

/// "0:07", "12:34", "1:02:03".
pub fn clock(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

impl Indicator {
    pub fn new(skin: Skin, stopped: Rc<Cell<bool>>) -> Self {
        Self {
            skin,
            started: None,
            ended_at: None,
            ending: false,
            stopped,
            done: false,
        }
    }

    /// Draws it in `ui`'s whole rect, `t` seconds into the recording.
    pub fn draw(&mut self, ui: &mut Ui, t: f64) {
        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        let radius = rect.height() / 2.0;
        let painter = ui.painter().clone();
        painter.rect(
            rect,
            CornerRadius::same(radius as u8),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        // The dot breathes, once every two seconds.
        let breath = 0.65 + 0.35 * (t * std::f64::consts::PI).cos().abs() as f32;
        let dot = pos2(rect.left() + radius, rect.center().y);
        painter.circle_filled(dot, 6.0, v.error_fg_color.gamma_multiply(breath));
        painter.text(
            pos2(dot.x + 16.0, rect.center().y),
            Align2::LEFT_CENTER,
            format!("Recording {}", clock(t as u64)),
            TextStyle::Body.resolve(ui.style()),
            v.text_color(),
        );

        // Stop: a square in a circle, at the right.
        let stop = Rect::from_center_size(
            pos2(rect.right() - radius, rect.center().y),
            vec2(30.0, 30.0),
        );
        let response = ui.interact(stop, ui.id().with("stop"), Sense::click());
        let fill = if response.hovered() {
            v.widgets.hovered.weak_bg_fill
        } else {
            v.widgets.inactive.weak_bg_fill
        };
        painter.circle_filled(stop.center(), 15.0, fill);
        painter.rect_filled(
            Rect::from_center_size(stop.center(), vec2(10.0, 10.0)),
            CornerRadius::same(2),
            v.text_color(),
        );
        if response.clicked() {
            self.stopped.set(true);
            self.done = true;
        }
    }
}

impl Shell for Indicator {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let now = ui.input(|i| i.time);
        let started = *self.started.get_or_insert(now);
        if self.ending && self.ended_at.is_none() {
            self.ended_at = Some(now);
        }
        let fade_in = ((now - started) / FADE).clamp(0.0, 1.0);
        let fade_out = self
            .ended_at
            .map_or(1.0, |at| 1.0 - ((now - at) / FADE).clamp(0.0, 1.0));
        if self.ended_at.is_some() && fade_out <= 0.0 {
            self.done = true;
            return;
        }
        // The clock and the dot move all the time it's up.
        ui.ctx().request_repaint();
        ui.set_opacity(fade_in.min(fade_out) as f32);
        self.draw(ui, now - started);
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// Shows it until its stdin closes or Stop is pressed; true for Stop.
pub fn run() -> Result<bool, String> {
    let (send, receive) = channel::channel::<()>();
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            let _ = std::io::stdin().lock().read_to_end(&mut Vec::new());
            let _ = send.send(());
        })
        .map_err(|e| e.to_string())?;
    let options = Options {
        namespace: "patina-screenshot".into(),
        layer: Layer::Overlay,
        anchor: Anchor::BOTTOM,
        size: SIZE,
        exclusive_zone: 0,
        margin: (GAP, GAP, GAP, GAP),
        keyboard: KeyboardInteractivity::None,
        per_output: false,
        toplevels: false,
        click_through: false,
        lock: false,
        window_fallback: false,
    };
    let stopped = Rc::new(Cell::new(false));
    let indicator = Indicator::new(piece::skin("patina-screenshot"), stopped.clone());
    let (host, event_loop) = Host::connect(options, indicator)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| match event {
            Event::Msg(()) | Event::Closed => host.shell.ending = true,
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)?;
    Ok(stopped.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws it, in software, as a PNG in the folder
    /// PATINA_SCREENSHOT_PREVIEW names.
    #[test]
    #[ignore]
    fn preview_the_indicator() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_SCREENSHOT_PREVIEW").unwrap());
        let (w, h) = (SIZE.0 as usize * 2, SIZE.1 as usize * 2);
        let ctx = egui::Context::default();
        let mut indicator = Indicator::new(Skin::default_skin(), Rc::default());
        indicator.skin.apply(&ctx);
        let mut painter = patina_shell::raster::Painter::default();
        let mut pixels = vec![0u32; w * h];
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(SIZE.0 as f32, SIZE.1 as f32),
                )),
                max_texture_side: Some(8192),
                ..Default::default()
            };
            ctx.set_pixels_per_point(2.0);
            let out = ctx.run_ui(input, |ui| indicator.draw(ui, 83.0));
            let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
            pixels.fill(0);
            painter.paint(&mut pixels, w, h, 2.0, &primitives, &out.textures_delta);
        }
        let rgba: Vec<u8> = pixels
            .iter()
            .flat_map(|p| {
                let [b, g, r, a] = p.to_le_bytes();
                [r, g, b, a]
            })
            .collect();
        image::save_buffer(
            dir.join("recording.png"),
            &rgba,
            w as u32,
            h as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }

    #[test]
    fn the_time_reads_as_a_clock() {
        assert_eq!(clock(7), "0:07");
        assert_eq!(clock(754), "12:34");
        assert_eq!(clock(3723), "1:02:03");
    }
}
