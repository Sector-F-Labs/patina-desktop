//! Three-finger drag, as macOS has it under Accessibility, but on by
//! default (ADR-0031): three fingers moving on the touchpad hold the left
//! button and move the pointer, so windows, text and files are dragged
//! without pressing the pad. Lifting lets go after a moment, or at once if
//! one finger moves the pointer on; putting three fingers back down within
//! it carries the drag on, so a long drag can be made in strokes.

use std::time::Duration;

use smithay::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};
use smithay::utils::{Logical, Point};

use crate::grabs::BTN_LEFT;
use crate::state::State;

/// How long after the fingers lift the button is still held.
const GRACE: Duration = Duration::from_millis(500);
/// How far one finger may move between strokes, in logical pixels, before
/// the drag drops: the first finger of the next stroke can land a moment
/// before the others.
const SLACK: f64 = 6.0;
const FINGERS: u32 = 3;

/// The drag's state; the left button is held from the first stroke's
/// start until the release after the last.
#[derive(Default)]
pub struct Drag {
    /// Three fingers are on the pad, dragging.
    stroking: bool,
    /// The left button is held for the drag.
    held: bool,
    /// The timer that lets go, between strokes.
    release: Option<RegistrationToken>,
    /// How far one finger has moved since the last stroke.
    drift: f64,
}

/// What a swipe's start does.
#[derive(Debug, PartialEq)]
enum Start {
    /// Not a drag: another number of fingers, or the setting off.
    Ignore,
    /// A new drag: press the button.
    Press,
    /// Back down in time: the drag goes on.
    Continue,
}

impl Drag {
    fn start(&mut self, fingers: u32, enabled: bool) -> Start {
        if !enabled || fingers != FINGERS {
            return Start::Ignore;
        }
        self.stroking = true;
        self.drift = 0.0;
        if self.held {
            Start::Continue
        } else {
            self.held = true;
            Start::Press
        }
    }

    /// One finger moved `by` between strokes: true once that's past the
    /// slack, and the drag should drop.
    fn drifted(&mut self, by: f64) -> bool {
        if !self.held || self.stroking {
            return false;
        }
        self.drift += by;
        self.drift > SLACK
    }

    /// The fingers lifted: true if a drag is waiting to be let go.
    fn lift(&mut self) -> bool {
        self.drift = 0.0;
        std::mem::take(&mut self.stroking) && self.held
    }
}

impl State {
    /// A touchpad swipe started with `fingers`; three start or carry on a
    /// drag.
    pub fn swipe_begin(&mut self, fingers: u32, time: u32) {
        match self
            .drag
            .start(fingers, self.config.touchpad.three_finger_drag)
        {
            Start::Ignore => {}
            Start::Press => self.pointer_button(BTN_LEFT, true, time),
            Start::Continue => {
                if let Some(token) = self.drag.release.take() {
                    self.loop_handle.remove(token);
                }
            }
        }
    }

    /// The fingers moved `delta`: the pointer follows, dragging.
    pub fn swipe_update(&mut self, delta: Point<f64, Logical>, time: u32) {
        if self.drag.stroking {
            self.move_pointer_by(delta, time);
        }
    }

    /// The fingers lifted: let go after a moment, unless they come back.
    pub fn swipe_end(&mut self) {
        if !self.drag.lift() {
            return;
        }
        let timer = Timer::from_duration(GRACE);
        match self.loop_handle.insert_source(timer, |_, _, state| {
            state.drag.release = None;
            state.end_drag();
            TimeoutAction::Drop
        }) {
            Ok(token) => self.drag.release = Some(token),
            Err(e) => {
                eprintln!("patina-compositor: three-finger drag: {e}");
                self.end_drag();
            }
        }
    }

    /// Lets go of a drag now, as when the grace runs out or a button is
    /// pressed during it.
    pub fn end_drag(&mut self) {
        if let Some(token) = self.drag.release.take() {
            self.loop_handle.remove(token);
        }
        self.drag.stroking = false;
        if std::mem::take(&mut self.drag.held) {
            let time = self.start.elapsed().as_millis() as u32;
            self.pointer_button(BTN_LEFT, false, time);
        }
    }

    /// One finger moved `delta` between strokes: past the slack, that's the
    /// pointer being moved, not the drag, so it drops at once.
    pub fn one_finger_moved(&mut self, delta: Point<f64, Logical>) {
        if self.drag.drifted(delta.x.abs() + delta.y.abs()) {
            self.end_drag();
        }
    }

    /// Whether a drag holds the button between strokes.
    pub fn drag_between_strokes(&self) -> bool {
        self.drag.held && !self.drag.stroking
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_fingers_press_once_and_carry_on_after_a_lift() {
        let mut drag = Drag::default();
        assert_eq!(drag.start(3, true), Start::Press);
        assert!(drag.lift());
        assert_eq!(drag.start(3, true), Start::Continue);
        assert!(drag.lift());
        assert!(!drag.lift(), "a second lift without a stroke is nothing");
    }

    #[test]
    fn one_finger_past_the_slack_between_strokes_drops_the_drag() {
        let mut drag = Drag::default();
        assert!(!drag.drifted(20.0), "no drag, nothing to drop");
        drag.start(3, true);
        assert!(!drag.drifted(20.0), "three fingers down: that's the drag");
        drag.lift();
        assert!(!drag.drifted(SLACK / 2.0), "a finger landing first");
        assert!(drag.drifted(SLACK));
        drag.start(3, true);
        drag.lift();
        assert!(!drag.drifted(SLACK / 2.0), "the slack starts again");
    }

    #[test]
    fn other_swipes_and_the_setting_off_are_ignored() {
        let mut drag = Drag::default();
        assert_eq!(drag.start(4, true), Start::Ignore);
        assert_eq!(drag.start(3, false), Start::Ignore);
        assert!(!drag.lift());
        assert!(!drag.held);
    }
}
