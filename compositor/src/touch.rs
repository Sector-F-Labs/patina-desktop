//! The touchscreen (ADR-0094): touches go to the app under them as wl_touch,
//! as GNOME gives them, so apps that handle touch there handle it here. A
//! touch going down focuses and raises what it lands on, as a click would.
//! Each finger's later motion goes to the surface it went down on (Smithay's
//! touch grab). Nothing is turned into pointer events: apps without touch,
//! the compositor's frames and the patina-shell pieces don't answer to it yet.

use smithay::backend::input::TouchSlot;
use smithay::input::touch::{DownEvent, MotionEvent, UpEvent};
use smithay::utils::{Logical, Point, SERIAL_COUNTER};

use crate::state::State;

impl State {
    pub fn touch_down(&mut self, slot: TouchSlot, pos: Point<f64, Logical>, time: u32) {
        let Some(touch) = self.seat.get_touch() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let under = self.surface_under(pos);
        // Locked, the touch goes to the locker and changes nothing else.
        if !self.locked() {
            // A layer surface takes the keyboard only if it wants it, and
            // leaves the windows' focus alone; anything else is a window
            // or the desktop.
            let layer = under.as_ref().and_then(|(s, _)| self.layer_for(s));
            if let Some(layer) = layer {
                if layer.can_receive_keyboard_focus() {
                    self.focus_layer(&layer, serial);
                }
            } else {
                let window = self.space.element_under(pos).map(|(w, _)| w.clone());
                self.focus(window, serial);
            }
        }
        touch.down(
            self,
            under,
            &DownEvent {
                slot,
                location: pos,
                serial,
                time,
            },
        );
    }

    pub fn touch_motion(&mut self, slot: TouchSlot, pos: Point<f64, Logical>, time: u32) {
        if let Some(touch) = self.seat.get_touch() {
            // The grab keeps each finger on the surface it went down on.
            touch.motion(
                self,
                None,
                &MotionEvent {
                    slot,
                    location: pos,
                    time,
                },
            );
        }
    }

    pub fn touch_up(&mut self, slot: TouchSlot, time: u32) {
        if let Some(touch) = self.seat.get_touch() {
            let serial = SERIAL_COUNTER.next_serial();
            touch.up(self, &UpEvent { slot, serial, time });
        }
    }

    pub fn touch_frame(&mut self) {
        if let Some(touch) = self.seat.get_touch() {
            touch.frame(self);
        }
    }

    pub fn touch_cancel(&mut self) {
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
    }
}
