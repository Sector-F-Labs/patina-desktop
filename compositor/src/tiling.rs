//! Tiling's mechanism (ADR-0049, ADR-0053): the compositor reports what
//! the pointer and keys do, and places windows and shows the preview where
//! it's told. The decisions are another process's: patina-tiler, which the
//! compositor starts and keeps running like the bar, or any app that takes
//! the Tiler name from it.
//!
//! `dbus.rs` serves `com.sectorflabs.Patina.Tiling`: it tells the bus about
//! every drag and Super+arrow, and windows opening and closing, and lets a
//! caller list, place and move windows and show the preview.

pub mod dbus;

use std::sync::atomic::{AtomicU64, Ordering};

use smithay::{
    backend::renderer::element::{
        Kind,
        solid::{SolidColorBuffer, SolidColorRenderElement},
    },
    desktop::Window,
    reexports::calloop::LoopHandle,
    utils::{Logical, Point, Rectangle},
};

use crate::maximize::{Placement, Snap, inset, placement};
use crate::layout::View;
use crate::state::State;
use dbus::{Event, Rect, Request, WindowRow};

/// The preview's colour: a translucent blue, premultiplied.
const PREVIEW: [f32; 4] = [0.35 * 0.3, 0.55 * 0.3, 0.9 * 0.3, 0.3];

#[derive(Default)]
pub struct Tiling {
    service: Option<dbus::Service>,
    /// A tiler owns the Tiler name.
    tiler: bool,
    /// Where a drop would put the window, shown over the desktop.
    preview: Option<(Rectangle<i32, Logical>, SolidColorBuffer)>,
    /// The work area tilers were last told of.
    announced: Option<Rectangle<i32, Logical>>,
}

impl Tiling {
    fn emit(&self, event: Event) {
        if let Some(service) = &self.service {
            service.emit(event);
        }
    }

    /// Tells the tiler of Super with an arrow on `window`.
    pub fn snap_requested(&self, window: &Window, snap: Snap) {
        let direction = match snap {
            Snap::Up => "up",
            Snap::Down => "down",
            Snap::Left => "left",
            Snap::Right => "right",
        };
        self.emit(Event::Snap(id(window), direction));
    }
}

/// Marks a window whose opening was told, so it's told once.
struct Shown;

/// A window's number on the bus, for as long as it's open.
struct Id(u64);

pub fn id(window: &Window) -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    window
        .user_data()
        .insert_if_missing(|| Id(NEXT.fetch_add(1, Ordering::Relaxed)));
    window.user_data().get::<Id>().expect("inserted above").0
}

fn rect(r: Rectangle<i32, Logical>) -> Rect {
    (r.loc.x, r.loc.y, r.size.w, r.size.h)
}

fn rectangle((x, y, w, h): Rect) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

/// How a placement is named on the bus.
fn placement_name(p: Option<Placement>) -> &'static str {
    match p {
        None => "",
        Some(Placement::Maximized) => "maximized",
        Some(Placement::Fill) => "fill",
        Some(Placement::Rect(_)) => "rect",
    }
}

impl State {
    /// Serves the interface, and starts watching for a tiler.
    pub fn start_tiling(&mut self, handle: &LoopHandle<'static, State>) {
        match dbus::Service::start(handle) {
            Ok(service) => self.tiling.service = Some(service),
            Err(e) => eprintln!("patina-compositor: tiling isn't on D-Bus: {e}"),
        }
    }

    /// The preview for this frame, while a drop would tile.
    pub fn preview_element(&self, view: View) -> Option<SolidColorRenderElement> {
        let (area, buffer) = self.tiling.preview.as_ref()?;
        let scale = view.scale;
        Some(SolidColorRenderElement::from_buffer(
            buffer,
            (area.loc - view.area.loc).to_physical_precise_round(scale),
            scale,
            1.0,
            Kind::Unspecified,
        ))
    }

    fn show_preview(&mut self, area: Option<Rectangle<i32, Logical>>) {
        if self.tiling.preview.as_ref().map(|(a, _)| *a) == area {
            return;
        }
        self.tiling.preview = area.map(|a| (a, SolidColorBuffer::new(a.size, PREVIEW)));
        self.needs_redraw = true;
    }

    /// A window started moving with the pointer.
    pub fn drag_started(&mut self, window: &Window) {
        self.tiling.emit(Event::DragStarted(id(window)));
    }

    /// The pointer moved to `pointer`, dragging `window`.
    pub fn drag_moved(&mut self, window: &Window, pointer: Point<f64, Logical>) {
        // The tiler works on the monitor the pointer is on. `pointer` is
        // passed in: this runs inside the pointer's grab, which holds its
        // lock, so the location can't be read from the handle here.
        let area = self.work_area_on(&self.output_under(pointer).clone());
        self.announce_work_area(inset(area, self.config.gap()));
        self.tiling
            .emit(Event::DragMotion(id(window), pointer.x, pointer.y));
    }

    /// `window` was let go of with the pointer at `pointer`; the tiler
    /// places it if it was dropped somewhere that tiles.
    pub fn drag_ended(&mut self, window: &Window, pointer: Point<f64, Logical>) {
        self.tiling
            .emit(Event::DragEnded(id(window), pointer.x, pointer.y));
        self.show_preview(None);
        // It may have come to rest on a monitor with another scale.
        self.update_preferred_scale();
    }

    /// Tells tilers tiled `window` is now at `rect`, after the line beside
    /// it was dragged.
    pub fn window_resized(&mut self, window: &Window, rect: Rectangle<i32, Logical>) {
        self.tiling
            .emit(Event::Resized(id(window), self::rect(rect)));
    }

    /// The screen a tiler draws its drop zones on: the pointer's monitor,
    /// pushed out past the edges another monitor touches, so the pointer
    /// can pass to it without tiling at the seam.
    pub fn tiler_screen(&self) -> Rectangle<i32, Logical> {
        const EDGE: i32 = 8;
        let mut screen = self
            .space
            .output_geometry(self.pointer_output())
            .unwrap_or_else(|| self.screen());
        let others: Vec<_> = self
            .outputs
            .iter()
            .filter_map(|o| self.space.output_geometry(o))
            .filter(|g| *g != screen)
            .collect();
        let touches = |a: i32, b: i32, c: i32, d: i32| a < d && c < b;
        let (l, t, r, b) = (
            screen.loc.x,
            screen.loc.y,
            screen.loc.x + screen.size.w,
            screen.loc.y + screen.size.h,
        );
        let (mut dl, mut dt, mut dr) = (0, 0, 0);
        for g in &others {
            let (gl, gt, gr, gb) = (g.loc.x, g.loc.y, g.loc.x + g.size.w, g.loc.y + g.size.h);
            if gr == l && touches(t, b, gt, gb) {
                dl = EDGE;
            }
            if gl == r && touches(t, b, gt, gb) {
                dr = EDGE;
            }
            if gb == t && touches(l, r, gl, gr) {
                dt = EDGE;
            }
        }
        screen.loc.x -= dl;
        screen.loc.y -= dt;
        screen.size.w += dl + dr;
        screen.size.h += dt;
        screen
    }

    /// Tells tilers the work area changed, if it did.
    pub fn announce_work_area(&mut self, area: Rectangle<i32, Logical>) {
        if self.tiling.announced != Some(area) {
            self.tiling.announced = Some(area);
            self.tiling.emit(Event::WorkArea(rect(area)));
        }
    }

    /// On a Wayland window's commit: the first time it has something to
    /// show, its app id and size known by then, tells tilers it opened, so
    /// one can put it where its app's window was (ADR-0065). A window
    /// that asked for fullscreen first isn't told of.
    pub fn window_shown(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        if window.geometry().size.is_empty() || !window.user_data().insert_if_missing(|| Shown) {
            return;
        }
        if self.fullscreen.as_ref() == Some(window) {
            return;
        }
        let Some(mut location) = self.space.element_location(window) else {
            return;
        };
        // Its frame, if it took one (ADR-0085), goes where it was put, the
        // window below.
        if placement(window).is_none() && self.framed(window) {
            location.y += crate::frames::TITLE;
            self.place(window, location);
        }
        let (_, app_id) = crate::toplevels::describe(toplevel.wl_surface());
        let outer = crate::frames::outer(window, Rectangle::new(location, window.geometry().size));
        self.tiling.emit(Event::Opened(
            id(window),
            app_id,
            placement_name(placement(window)).to_string(),
            rect(outer),
        ));
    }

    /// Tells tilers `window` is closing, with where it floats (or floated
    /// before it was minimised), or floated before it was placed or went
    /// fullscreen.
    pub fn window_closed(&mut self, window: &Window) {
        let Some(toplevel) = window.toplevel() else {
            return;
        };
        let floated = crate::fullscreen::was(window)
            .or_else(|| crate::maximize::before(window))
            .or_else(|| {
                let location = self
                    .space
                    .element_location(window)
                    .or_else(|| self.minimized.location(window))?;
                Some(Rectangle::new(location, window.geometry().size))
            });
        let Some(floated) = floated else {
            return;
        };
        let floated = crate::frames::outer(window, floated);
        let (_, app_id) = crate::toplevels::describe(toplevel.wl_surface());
        self.tiling
            .emit(Event::Closed(id(window), app_id, rect(floated)));
    }

    fn window_by_id(&self, wanted: u64) -> Option<Window> {
        self.space.elements().find(|w| id(w) == wanted).cloned()
    }

    fn windows(&self) -> Vec<WindowRow> {
        self.space
            .elements()
            .filter(|w| w.toplevel().is_some())
            .filter_map(|w| {
                let outer = self.outer_rect(w)?;
                let toplevel = w.toplevel()?;
                let (title, app_id) = crate::toplevels::describe(toplevel.wl_surface());
                Some((
                    id(w),
                    app_id,
                    title,
                    placement_name(placement(w)).to_string(),
                    rect(outer),
                ))
            })
            .collect()
    }

    pub fn tiling_request(&mut self, request: Request) {
        match request {
            Request::Tiler(owned) => {
                if owned != self.tiling.tiler {
                    eprintln!(
                        "patina-compositor: {}",
                        if owned {
                            "a tiler decides tiling"
                        } else {
                            "no tiler: windows don't tile until one starts"
                        }
                    );
                }
                self.tiling.tiler = owned;
                // A drag's preview is the tiler's; a new one shows its own.
                self.show_preview(None);
            }
            Request::Windows(reply) => {
                let _ = reply.send(self.windows());
            }
            Request::WorkArea(reply) => {
                let _ = reply.send(rect(self.tiling_area()));
            }
            Request::Gap(reply) => {
                let _ = reply.send(self.config.gap());
            }
            Request::Screen(reply) => {
                let _ = reply.send(rect(self.tiler_screen()));
            }
            Request::Place { id, area, reply } => {
                let result = match self.window_by_id(id) {
                    None => Err(format!("no window {id}")),
                    Some(_) if area.2 <= 0 || area.3 <= 0 => {
                        Err(format!("{area:?}: the width and height must be positive"))
                    }
                    Some(window) if window.toplevel().is_none() => Err(format!(
                        "window {id} is an X11 window, which can't be tiled yet"
                    )),
                    Some(window) => {
                        let area = rectangle(area);
                        // The monitor the area is on.
                        let centre = area.loc.to_f64() + area.size.to_f64().downscale(2.0).to_point();
                        let on = self.output_under(centre).clone();
                        let work = self.work_area_on(&on);
                        let placement = if area == inset(work, self.config.gap()) {
                            Placement::Fill
                        } else {
                            Placement::Rect(area)
                        };
                        self.place_as_on(&window, placement, centre);
                        Ok(())
                    }
                };
                let _ = reply.send(result);
            }
            Request::Move { id, at, reply } => {
                let result = match self.window_by_id(id) {
                    None => Err(format!("no window {id}")),
                    Some(window) if placement(&window).is_some() => {
                        Err(format!("window {id} is placed; restore it first"))
                    }
                    Some(window) => {
                        // `at` is where its frame goes, if it has one.
                        let at = Point::from(at) + Point::from((0, crate::frames::top(&window)));
                        self.place(&window, at);
                        Ok(())
                    }
                };
                let _ = reply.send(result);
            }
            Request::Restore { id, reply } => {
                let result = match self.window_by_id(id) {
                    Some(window) => {
                        self.unmaximize(&window, None);
                        Ok(())
                    }
                    None => Err(format!("no window {id}")),
                };
                let _ = reply.send(result);
            }
            Request::Preview(area) => {
                self.show_preview(area.filter(|a| a.2 > 0 && a.3 > 0).map(rectangle));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_have_names_on_the_bus() {
        assert_eq!(placement_name(None), "");
        assert_eq!(placement_name(Some(Placement::Fill)), "fill");
        let rect = Rectangle::new((0, 0).into(), (640, 800).into());
        assert_eq!(placement_name(Some(Placement::Rect(rect))), "rect");
    }
}
