//! A notice: a pill with a line of text, shown for a couple of seconds and
//! faded away, as dictation's overlay is (ADR-0073) but for anything that has
//! one thing to say. It's a layer-shell surface that takes neither keyboard
//! nor pointer, so it never gets in the way. [`show`] blocks until it has
//! gone, so a program makes one by running itself with the text, or from a
//! thread.

use egui::{Align2, CornerRadius, FontId, StrokeKind, Ui};
use patina::skin::Skin;

use crate::host::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell};
use crate::piece;

/// Between the pill and the screen's edge, past the bar if the bar is there.
const GAP: i32 = 24;
/// The pill's height, in logical pixels.
const HEIGHT: u32 = 44;
/// Seconds to fade in, to stay, and to fade out.
const FADE_IN: f64 = 0.12;
const HOLD: f64 = 2.0;
const FADE_OUT: f64 = 0.2;

/// How opaque the pill is `elapsed` seconds after it appeared; `None` once it's gone.
pub fn opacity(elapsed: f64) -> Option<f32> {
    let out_at = FADE_IN + HOLD;
    if elapsed >= out_at + FADE_OUT {
        return None;
    }
    let fade_in = (elapsed / FADE_IN).clamp(0.0, 1.0);
    let fade_out = 1.0 - ((elapsed - out_at) / FADE_OUT).clamp(0.0, 1.0);
    Some(fade_in.min(fade_out) as f32)
}

/// The pill's width for `text`: the surface has to be sized before anything
/// can measure the text, so this is a generous guess per character.
pub fn width(text: &str) -> u32 {
    (text.chars().count() as u32 * 9 + 2 * HEIGHT).clamp(160, 640)
}

struct Toast {
    skin: Skin,
    text: String,
    shown_at: Option<f64>,
    done: bool,
}

impl Shell for Toast {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        if self.skin.follows_desktop() {
            patina::appearance::watch(ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, _: &str) {
        let now = ui.input(|i| i.time);
        let shown_at = *self.shown_at.get_or_insert(now);
        let Some(opacity) = opacity(now - shown_at) else {
            self.done = true;
            return;
        };
        ui.request_repaint();
        ui.set_opacity(opacity);
        let rect = ui.max_rect();
        let v = ui.visuals().clone();
        let painter = ui.painter();
        painter.rect(
            rect,
            CornerRadius::same((rect.height() / 2.0) as u8),
            v.panel_fill,
            v.window_stroke,
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            &self.text,
            FontId::proportional(15.0),
            v.text_color(),
        );
    }

    fn finished(&self) -> bool {
        self.done
    }
}

/// Shows `text` at the top centre of the screen until it has faded out.
/// `app` names the surface and picks the skin, as for the other pieces.
pub fn show(app: &str, text: &str) -> Result<(), String> {
    let options = Options {
        namespace: app.into(),
        layer: Layer::Overlay,
        anchor: Anchor::TOP,
        size: (width(text), HEIGHT),
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
    let toast = Toast {
        skin: piece::skin(app),
        text: text.to_string(),
        shown_at: None,
        done: false,
    };
    let (host, event_loop) = Host::connect(options, toast)?;
    host.run(event_loop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_fades_in_stays_and_fades_out() {
        assert_eq!(opacity(0.0), Some(0.0));
        assert_eq!(opacity(FADE_IN), Some(1.0));
        assert_eq!(opacity(1.0), Some(1.0));
        let mid = opacity(FADE_IN + HOLD + FADE_OUT / 2.0).unwrap();
        assert!((0.4..0.6).contains(&mid));
        assert_eq!(opacity(FADE_IN + HOLD + FADE_OUT), None);
    }

    #[test]
    fn the_pill_fits_its_text_within_limits() {
        assert_eq!(width("On"), 160);
        assert!(width("Game mode on") > 160);
        assert_eq!(width(&"x".repeat(200)), 640);
    }
}
