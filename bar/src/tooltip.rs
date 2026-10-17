//! The bar's tooltips (ADR-0057). The bar is too thin to hold a tooltip,
//! so what's hovered is noted (`hover`), and after a moment a tooltip is
//! shown in `patina-menu --tooltip`, a surface of its own just past the
//! bar's edge, which closes when the bar closes its input.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use egui::{Id, Rect, Response, Ui};

/// How long something is hovered before its tooltip shows, as egui's.
pub const DELAY: Duration = Duration::from_millis(500);

/// Where this frame's hovered tooltip is kept, in each output's context.
fn key() -> Id {
    Id::new("patina-bar-tooltip")
}

/// Notes `text` as the tooltip for `response`, if it's hovered.
pub fn hover(ui: &Ui, response: &Response, text: impl Into<String>) {
    if response.hovered() {
        note(ui, response.rect, text);
    }
}

/// Notes `text` as the tooltip for what's hovered at `rect`.
pub fn note(ui: &Ui, rect: Rect, text: impl Into<String>) {
    let tip = (rect, text.into());
    ui.ctx().data_mut(|d| d.insert_temp(key(), tip));
}

/// The tooltip noted this frame, taken.
pub fn take(ui: &Ui) -> Option<(Rect, String)> {
    ui.ctx().data_mut(|d| {
        let tip = d.get_temp::<(Rect, String)>(key());
        d.remove::<(Rect, String)>(key());
        tip
    })
}

/// A tooltip: on which output, beside what, and what it says.
#[derive(Debug, Clone, PartialEq)]
pub struct Tip {
    pub output: String,
    pub rect: Rect,
    pub text: String,
}

/// What to do after a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Show(Tip),
    Hide,
    /// Draw again then, to show the tooltip once it's been hovered long
    /// enough.
    Wait(Duration),
    Nothing,
}

/// What's hovered, and whether its tooltip shows.
#[derive(Default)]
pub struct Tips {
    hovering: Option<(Tip, Instant)>,
    shown: bool,
}

impl Tips {
    /// After a frame of `output`, in which `hovered` was hovered (or
    /// nothing): what to do with the tooltip.
    pub fn frame(&mut self, output: &str, hovered: Option<(Rect, String)>, now: Instant) -> Action {
        let Some((rect, text)) = hovered else {
            // Nothing hovered here: the tooltip goes, if it was here.
            let here = self
                .hovering
                .as_ref()
                .is_some_and(|(tip, _)| tip.output == output);
            if !here {
                return Action::Nothing;
            }
            self.hovering = None;
            return self.hide();
        };
        let tip = Tip {
            output: output.to_string(),
            rect,
            text,
        };
        if self.hovering.as_ref().map(|(t, _)| t) != Some(&tip) {
            self.hovering = Some((tip, now));
            if self.shown {
                return self.hide();
            }
        }
        let (tip, since) = self.hovering.as_ref().expect("set above");
        if self.shown {
            return Action::Nothing;
        }
        let waited = now.duration_since(*since);
        if waited >= DELAY {
            self.shown = true;
            Action::Show(tip.clone())
        } else {
            Action::Wait(DELAY - waited)
        }
    }

    fn hide(&mut self) -> Action {
        if std::mem::take(&mut self.shown) {
            Action::Hide
        } else {
            Action::Nothing
        }
    }
}

/// How far past the bar's edge a tooltip shows.
const SPACE: f32 = 4.0;

/// patina-menu's arguments for `text`, centred on X from the screen's
/// left, past the bar, which reaches `from_edge` in from its edge.
pub fn args(text: &str, x: f32, from_edge: f32, edge: &str) -> Vec<String> {
    vec![
        "--tooltip".into(),
        text.into(),
        "--at".into(),
        format!("{:.0},{:.0}", x, from_edge + SPACE),
        "--edge".into(),
        edge.into(),
    ]
}

/// A tooltip showing: patina-menu, which closes with its input.
pub struct Shown(Option<Child>);

impl Shown {
    pub fn start(args: &[String]) -> Option<Self> {
        match Command::new("patina-menu")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(child) => Some(Self(Some(child))),
            Err(e) => {
                eprintln!("patina-bar: can't show a tooltip with patina-menu: {e}");
                None
            }
        }
    }
}

impl Drop for Shown {
    /// Closes its input, so it goes, and reaps it on a thread.
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            drop(child.stdin.take());
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    fn rect(x: f32) -> Rect {
        Rect::from_min_size(pos2(x, 0.0), egui::vec2(40.0, 41.0))
    }

    #[test]
    fn a_tooltip_shows_after_the_delay_and_goes_when_the_pointer_does() {
        let mut tips = Tips::default();
        let t0 = Instant::now();
        let hover = || Some((rect(100.0), "Wi-Fi 72%".to_string()));
        assert_eq!(tips.frame("A", hover(), t0), Action::Wait(DELAY));
        let half = t0 + DELAY / 2;
        assert_eq!(tips.frame("A", hover(), half), Action::Wait(DELAY / 2));
        let Action::Show(tip) = tips.frame("A", hover(), t0 + DELAY) else {
            panic!("shown after the delay");
        };
        assert_eq!(tip.text, "Wi-Fi 72%");
        assert_eq!(tips.frame("A", hover(), t0 + DELAY * 2), Action::Nothing);
        assert_eq!(tips.frame("A", None, t0 + DELAY * 3), Action::Hide);
        assert_eq!(tips.frame("A", None, t0 + DELAY * 4), Action::Nothing);
    }

    #[test]
    fn moving_to_another_item_starts_again() {
        let mut tips = Tips::default();
        let t0 = Instant::now();
        tips.frame("A", Some((rect(100.0), "Wi-Fi".into())), t0);
        tips.frame("A", Some((rect(100.0), "Wi-Fi".into())), t0 + DELAY);
        let later = t0 + DELAY * 2;
        assert_eq!(
            tips.frame("A", Some((rect(140.0), "Volume 40%".into())), later),
            Action::Hide
        );
        assert_eq!(
            tips.frame("A", Some((rect(140.0), "Volume 40%".into())), later),
            Action::Wait(DELAY)
        );
    }

    #[test]
    fn another_outputs_frame_leaves_it_alone() {
        let mut tips = Tips::default();
        let t0 = Instant::now();
        tips.frame("A", Some((rect(100.0), "Wi-Fi".into())), t0);
        tips.frame("A", Some((rect(100.0), "Wi-Fi".into())), t0 + DELAY);
        assert_eq!(tips.frame("B", None, t0 + DELAY), Action::Nothing);
    }

    #[test]
    fn the_tooltip_shows_past_the_bar() {
        assert_eq!(
            args("Muted", 412.3, 49.0, "bottom"),
            ["--tooltip", "Muted", "--at", "412,53", "--edge", "bottom"]
        );
    }
}
