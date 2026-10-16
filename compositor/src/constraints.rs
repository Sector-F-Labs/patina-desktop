//! Mouse-look for games: relative-pointer tells a client how far the mouse
//! moved, whatever the pointer's position, and pointer-constraints lets it
//! lock the pointer in place or confine it to its window. Xwayland needs
//! both to give an X11 game the motion it reads after warping the pointer
//! to the window's centre; without them the game sees the jump from the
//! centre back to the real pointer, and aims the wrong way.

use smithay::{
    input::pointer::{PointerHandle, RelativeMotionEvent},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
    wayland::{
        compositor::RegionAttributes,
        pointer_constraints::{
            PointerConstraint, PointerConstraintsHandler, with_pointer_constraint,
        },
    },
};

use crate::state::State;

/// The active constraint on the surface under the pointer.
enum Constraint {
    Locked,
    Confined(Option<RegionAttributes>),
}

impl State {
    /// A mouse or touchpad moved by `delta`: sent as relative motion, then
    /// the pointer moves, unless the client under it has locked it, or
    /// confined it and this would leave the region.
    pub fn pointer_moved(
        &mut self,
        delta: Point<f64, Logical>,
        delta_unaccel: Point<f64, Logical>,
        utime: u64,
        time: u32,
    ) {
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        let location = pointer.current_location();
        let under = self.surface_under(location);
        let constraint = under
            .as_ref()
            .and_then(|(surface, origin)| active_constraint(surface, &pointer, location - *origin));
        pointer.relative_motion(
            self,
            under.clone(),
            &RelativeMotionEvent {
                delta,
                delta_unaccel,
                utime,
            },
        );
        match (constraint, under) {
            (Some(Constraint::Locked), _) => {
                pointer.frame(self);
                return;
            }
            (Some(Constraint::Confined(region)), Some((surface, origin))) => {
                let target = self.pointer_target(delta);
                let inside = self
                    .surface_under(target)
                    .is_some_and(|(s, _)| s == surface)
                    && region.is_none_or(|r| r.contains((target - origin).to_i32_round()));
                if !inside {
                    pointer.frame(self);
                    return;
                }
            }
            _ => {}
        }
        self.move_pointer_by(delta, time);
        self.activate_constraint(&pointer);
    }

    /// Activates the constraint of the surface the pointer is now over, if
    /// it has one and the pointer is inside its region.
    fn activate_constraint(&mut self, pointer: &PointerHandle<Self>) {
        let location = pointer.current_location();
        let Some((surface, origin)) = self.surface_under(location) else {
            return;
        };
        with_pointer_constraint(&surface, pointer, |constraint| {
            if let Some(constraint) = constraint
                && !constraint.is_active()
                && constraint
                    .region()
                    .is_none_or(|r| r.contains((location - origin).to_i32_round()))
            {
                constraint.activate();
            }
        });
    }
}

fn active_constraint(
    surface: &WlSurface,
    pointer: &PointerHandle<State>,
    local: Point<f64, Logical>,
) -> Option<Constraint> {
    with_pointer_constraint(surface, pointer, |constraint| {
        let constraint = constraint.filter(|c| c.is_active())?;
        // Outside its region, a constraint doesn't hold.
        if !constraint
            .region()
            .is_none_or(|r| r.contains(local.to_i32_round()))
        {
            return None;
        }
        Some(match &*constraint {
            PointerConstraint::Locked(_) => Constraint::Locked,
            PointerConstraint::Confined(confined) => {
                Constraint::Confined(confined.region().cloned())
            }
        })
    })
}

impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        if pointer.current_focus().as_ref() == Some(surface) {
            self.activate_constraint(pointer);
        }
    }

    /// Where the client draws its cursor while it's locked: the pointer
    /// goes there, so it's in the same place when the lock ends.
    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        let active =
            with_pointer_constraint(surface, pointer, |c| c.is_some_and(|c| c.is_active()));
        let origin = self
            .window_for(surface)
            .and_then(|w| self.space.element_location(&w));
        if active && let Some(origin) = origin {
            pointer.set_location(origin.to_f64() + location);
        }
    }
}

smithay::delegate_pointer_constraints!(State);
smithay::delegate_relative_pointer!(State);
