//! The overlay shown while dictating (ADR-0073): a pill at the bottom or top
//! of the screen with a waveform that follows the voice, then a spinner
//! while the words are transcribed. It's `patina-dictate --overlay EDGE`, a
//! layer-shell surface on patina-shell that takes neither keyboard nor
//! pointer, started by the daemon at each press. It reads its stdin a line
//! at a time: a number is the loudness now (RMS), `transcribing` ends the
//! listening, and the end of the input fades it out. So it can be tried on
//! its own, with nothing listening:
//!
//! ```sh
//! (for i in $(seq 60); do echo 0.0$((RANDOM % 9)); sleep 0.03; done;
//!  echo transcribing; sleep 1) | patina-dictate --overlay bottom
//! ```

use std::collections::VecDeque;
use std::io::BufRead;

use egui::{Color32, CornerRadius, Rect, Spinner, StrokeKind, Ui, pos2, vec2};
use patina::skin::Skin;
use patina_shell::calloop::channel::{self, Event};
use patina_shell::piece;
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};

use crate::config::Edge;

/// The pill, in logical pixels.
pub const SIZE: (u32, u32) = (240, 44);
/// Between it and the screen's edge, past the bar if the bar is there.
const GAP: i32 = 24;
/// Bars in the waveform: one per level read, a thirtieth of a second each.
const BARS: usize = 40;
/// Fade in, and out once the input ends, in seconds.
const FADE_IN: f64 = 0.12;
const FADE_OUT: f64 = 0.2;
/// What the bars settle to while transcribing.
const RESTING: f32 = 0.08;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Listening,
    Transcribing,
}

/// A line of the overlay's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    Level(f32),
    Transcribing,
}

/// A line, or `None` for one it doesn't know.
pub fn parse_line(line: &str) -> Option<Message> {
    match line.trim() {
        "transcribing" => Some(Message::Transcribing),
        level => level.parse::<f32>().ok().map(Message::Level),
    }
}

/// How tall a bar is for a loudness (RMS), 0 to 1: speech is around 0.02 to
/// 0.2, so a square root spreads it over the height, and the room's hum
/// stays near the bottom.
pub fn height(rms: f32) -> f32 {
    ((rms - 0.003).max(0.0) * 12.0).sqrt().clamp(0.0, 1.0)
}

pub struct Overlay {
    skin: Skin,
    phase: Phase,
    /// Bar heights, 0 to 1, the newest last.
    levels: VecDeque<f32>,
    /// In egui's time, set on the first frame, and when the input ended.
    shown_at: Option<f64>,
    ended_at: Option<f64>,
    ending: bool,
    done: bool,
}

impl Overlay {
    pub fn new(skin: Skin) -> Self {
        Self {
            skin,
            phase: Phase::Listening,
            levels: VecDeque::with_capacity(BARS),
            shown_at: None,
            ended_at: None,
            ending: false,
            done: false,
        }
    }

    pub fn take(&mut self, message: Message) {
        match message {
            Message::Level(rms) if self.phase == Phase::Listening => {
                if self.levels.len() == BARS {
                    self.levels.pop_front();
                }
                self.levels.push_back(height(rms));
            }
            Message::Level(_) => {}
            Message::Transcribing => self.phase = Phase::Transcribing,
        }
    }

    /// The input ended: fade out.
    pub fn end(&mut self) {
        self.ending = true;
    }

    fn opacity(&self, now: f64) -> f32 {
        let shown = now - self.shown_at.unwrap_or(now);
        let fade_in = (shown / FADE_IN).clamp(0.0, 1.0);
        let fade_out = self
            .ended_at
            .map_or(1.0, |at| 1.0 - ((now - at) / FADE_OUT).clamp(0.0, 1.0));
        fade_in.min(fade_out) as f32
    }

    /// Draws it in `ui`'s whole rect, at egui's time `now`.
    pub fn draw(&mut self, ui: &mut Ui, now: f64) {
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

        // At the left: a dot that swells with the voice, or a spinner.
        let mark = pos2(rect.left() + radius, rect.center().y);
        match self.phase {
            Phase::Listening => {
                let now_level = self.levels.back().copied().unwrap_or(0.0);
                painter.circle_filled(mark, 5.0 + 3.0 * now_level, v.error_fg_color);
            }
            Phase::Transcribing => {
                ui.put(
                    Rect::from_center_size(mark, vec2(16.0, 16.0)),
                    Spinner::new().size(16.0),
                );
            }
        }

        // The waveform: bars mirrored about the middle, newest at the right.
        let wave = Rect::from_min_max(
            pos2(rect.left() + 2.0 * radius, rect.top() + 10.0),
            pos2(rect.right() - radius, rect.bottom() - 10.0),
        );
        let step = wave.width() / BARS as f32;
        let bar = (step * 0.55).max(1.0);
        let missing = BARS - self.levels.len();
        if self.phase == Phase::Transcribing {
            // The bars settle, and a brighter band sweeps along them.
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            for level in &mut self.levels {
                *level = RESTING + (*level - RESTING) * (-dt * 8.0).exp();
            }
        }
        let sweep = (now * 1.2).fract() as f32;
        for i in 0..BARS {
            let level = i
                .checked_sub(missing)
                .and_then(|j| self.levels.get(j).copied())
                .unwrap_or(0.0);
            let height = 2.0 + level * (wave.height() - 2.0);
            let x = wave.left() + step * (i as f32 + 0.5);
            let colour = match self.phase {
                Phase::Listening => v.text_color(),
                Phase::Transcribing => {
                    let away = (i as f32 / BARS as f32 - sweep).abs();
                    let near = (1.0 - away * 5.0).clamp(0.0, 1.0);
                    lerp(v.weak_text_color(), v.text_color(), near)
                }
            };
            painter.rect_filled(
                Rect::from_center_size(pos2(x, wave.center().y), vec2(bar, height)),
                CornerRadius::same((bar / 2.0) as u8),
                colour,
            );
        }
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t)
}

impl Shell for Overlay {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let now = ui.input(|i| i.time);
        self.shown_at.get_or_insert(now);
        if self.ending && self.ended_at.is_none() {
            self.ended_at = Some(now);
        }
        let opacity = self.opacity(now);
        if self.ended_at.is_some() && opacity <= 0.0 {
            self.done = true;
            return;
        }
        // It moves all the time it's up.
        ui.ctx().request_repaint();
        ui.set_opacity(opacity);
        self.draw(ui, now);
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// Shows the overlay at `edge` until its input ends.
pub fn run(edge: Edge) -> Result<(), String> {
    let (send, receive) = channel::channel::<Option<Message>>();
    std::thread::Builder::new()
        .name("stdin".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Some(message) = parse_line(&line)
                    && send.send(Some(message)).is_err()
                {
                    return;
                }
            }
            let _ = send.send(None);
        })
        .map_err(|e| e.to_string())?;

    let anchor = match edge {
        Edge::Bottom => Anchor::BOTTOM,
        Edge::Top => Anchor::TOP,
    };
    let options = Options {
        namespace: "patina-dictate".into(),
        layer: Layer::Overlay,
        anchor,
        size: SIZE,
        // 0: placed clear of the bar's exclusive zone, if it's on this edge.
        exclusive_zone: 0,
        margin: (GAP, GAP, GAP, GAP),
        keyboard: KeyboardInteractivity::None,
        per_output: false,
        toplevels: false,
        click_through: true,
        lock: false,
        window_fallback: false,
    };
    let overlay = Overlay::new(piece::skin("patina-dictate"));
    let (host, event_loop) = Host::connect(options, overlay)?;
    event_loop
        .handle()
        .insert_source(receive, |event, _, host| match event {
            Event::Msg(Some(message)) => host.shell.take(message),
            Event::Msg(None) | Event::Closed => host.shell.end(),
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    #[test]
    fn lines_are_levels_or_transcribing() {
        assert_eq!(parse_line("0.05"), Some(Message::Level(0.05)));
        assert_eq!(parse_line(" transcribing\n"), Some(Message::Transcribing));
        assert_eq!(parse_line("loud"), None);
    }

    #[test]
    fn quiet_is_low_and_shouting_is_full() {
        assert_eq!(height(0.0), 0.0);
        assert!(height(0.002) < 0.01, "the room's hum");
        assert!((0.3..0.9).contains(&height(0.03)), "speech");
        assert_eq!(height(0.5), 1.0);
    }

    #[test]
    fn the_waveform_keeps_its_last_bars_and_stops_on_transcribing() {
        let mut overlay = Overlay::new(Skin::default_skin());
        for _ in 0..BARS + 5 {
            overlay.take(Message::Level(0.05));
        }
        assert_eq!(overlay.levels.len(), BARS);
        overlay.take(Message::Transcribing);
        overlay.take(Message::Level(0.5));
        assert_eq!(overlay.phase, Phase::Transcribing);
        assert_eq!(overlay.levels.back(), Some(&height(0.05)));
    }

    /// Draws it listening and transcribing, in each built-in skin, in software,
    /// as PNGs in the folder PATINA_DICTATE_PREVIEW names, to look at and for
    /// the README: `PATINA_DICTATE_PREVIEW=/tmp/p cargo test -p patina-dictate
    /// preview -- --ignored`.
    #[test]
    #[ignore]
    fn preview_the_overlay() {
        let dir = PathBuf::from(std::env::var("PATINA_DICTATE_PREVIEW").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let scale = 3.0;
        let (w, h) = (
            (SIZE.0 as f32 * scale) as usize,
            (SIZE.1 as f32 * scale) as usize,
        );
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        // A voice: syllables, louder and quieter.
        let voice: Vec<f32> = (0..BARS)
            .map(|i| {
                let t = i as f32 * 0.4;
                (0.004 + 0.06 * (t.sin() * (t * 0.37).cos()).abs()).max(0.0)
            })
            .collect();
        for (skin, (phase, transcribing)) in skins
            .iter()
            .flat_map(|s| [(s, ("listening", false)), (s, ("transcribing", true))])
        {
            let name = format!("{}-{phase}", skin.name);
            let ctx = egui::Context::default();
            let mut overlay = Overlay::new(skin.clone());
            overlay.skin.apply(&ctx);
            for &level in &voice {
                overlay.take(Message::Level(level));
            }
            if transcribing {
                overlay.take(Message::Transcribing);
            }
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for frame in 0..6 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(SIZE.0 as f32, SIZE.1 as f32),
                    )),
                    time: Some(0.3 + frame as f64 * 0.02),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                ctx.set_pixels_per_point(scale);
                let out = ctx.run_ui(input, |ui| overlay.draw(ui, 0.3));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                pixels.fill(0);
                painter.paint(&mut pixels, w, h, scale, &primitives, &out.textures_delta);
                std::thread::sleep(Duration::from_millis(20));
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            image::save_buffer(
                dir.join(format!("{name}.png")),
                &rgba,
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
}
