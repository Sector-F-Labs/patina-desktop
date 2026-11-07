//! The compositor's server side: the Wayland globals clients see, and how
//! windows and layer surfaces are placed. Windows float (ADR-0007): each is
//! placed once, cascading inside the area the layer surfaces leave free, and
//! a click focuses and raises it.

use std::{ffi::OsString, sync::Arc, time::Instant};

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{
        LayerSurface, PopupKind, PopupManager, Space, Window, WindowSurfaceType,
        find_popup_root_surface, get_popup_toplevel_coords, layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        pointer::{AxisFrame, ButtonEvent, CursorImageStatus, Focus, GrabStartData, MotionEvent},
    },
    output::Output,
    reexports::{
        calloop::{
            EventLoop, Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic,
        },
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface, with_states,
        },
        output::{OutputHandler, OutputManagerState},
        selection::{
            SelectionHandler,
            data_device::{
                DataDeviceHandler, DataDeviceState, ServerDndGrabHandler, set_data_device_focus,
            },
        },
        shell::{
            wlr_layer::{
                KeyboardInteractivity, Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData,
                WlrLayerShellHandler, WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};

use crate::grabs::{self, ResizeGrab};
use crate::nested::Nested;
use crate::toplevels::ForeignToplevels;

/// Where the first window goes, inside the free area, and how far each
/// next one steps down and right.
const CASCADE_START: i32 = 48;
const CASCADE_STEP: i32 = 32;

pub struct State {
    pub start: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    /// For timers, such as the end of a three-finger drag.
    pub loop_handle: LoopHandle<'static, State>,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub output: Output,
    /// Every monitor, the primary (`output`) first (ADR-0121).
    pub outputs: Vec<Output>,
    /// The globals advertising `outputs`, to take back when one goes.
    output_globals: Vec<(Output, smithay::reexports::wayland_server::backend::GlobalId)>,
    /// Something changed on screen since the last frame.
    pub needs_redraw: bool,
    /// Quitting so patina-session starts the compositor again.
    pub restart: bool,
    placed: i32,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    /// Server-side frames for Wayland windows that ask (ADR-0085).
    _xdg_decoration_state: smithay::wayland::shell::xdg::decoration::XdgDecorationState,
    layer_shell_state: WlrLayerShellState,
    shm_state: ShmState,
    _output_manager_state: OutputManagerState,
    seat_state: SeatState<State>,
    data_device_state: DataDeviceState,
    /// The clipboard for programs without a window (wl-copy, clipboard
    /// managers), in wlroots' protocol and the standard one (ADR-0059).
    data_control_state: smithay::wayland::selection::wlr_data_control::DataControlState,
    ext_data_control_state: smithay::wayland::selection::ext_data_control::DataControlState,
    pub seat: Seat<State>,

    /// The window on the parent desktop that this compositor draws into.
    pub nested: Option<Nested>,
    /// Or the window it draws into with OpenGL ES (ADR-0015).
    pub winit: Option<crate::winit::WinitData>,
    /// What the bar's window list has been told.
    pub toplevels: ForeignToplevels,
    /// The settings from config.toml in use (ADR-0019).
    pub config: crate::config::Config,
    /// Night Light (ADR-0039).
    pub night_light: crate::night_light::NightLight,
    /// The keyboard's layouts, as told to the bus (ADR-0068).
    pub layouts: crate::keyboard::Layouts,
    /// The compositor's own keys, from `[keys]` (ADR-0069).
    pub keys: crate::keys::Keys,
    /// The lock screen (ADR-0025).
    pub lock: crate::lock::Lock,
    pub session_lock: smithay::wayland::session_lock::SessionLockManagerState,
    /// The screen turning off without input, and what holds it on
    /// (ADR-0047).
    pub idle: crate::idle::Idle,
    _idle_inhibit: smithay::wayland::idle_inhibit::IdleInhibitManagerState,
    /// Locked, and a frame has been drawn since: the desktop is hidden.
    pub lock_shown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Super is down, and nothing else has been pressed since.
    pub super_tap: bool,
    /// Super keys sent on as Ctrl (ADR-0067).
    pub cmd: crate::cmd::Cmd,
    /// Keys sent on as others, still down (ADR-0076).
    pub remapped: crate::remap::Remapped,
    /// Caps Lock held as a layer (ADR-0086).
    pub caps: crate::caps::Caps,
    /// Frames drawn for X11 windows without their own (ADR-0077).
    pub frames: crate::frames::Frames,
    /// Text typed into the focused window for dictation (ADR-0071).
    pub typing: crate::typing::Typing,
    pub clipboard: crate::clipboard::Clipboard,
    /// A three-finger drag, holding the left button (ADR-0031).
    pub drag: crate::drag::Drag,
    /// The scale the backend would choose by itself: the panel's density on
    /// the hardware, the parent desktop's when nested.
    pub auto_scale: f64,
    _fractional_scale: smithay::wayland::fractional_scale::FractionalScaleManagerState,
    _viewporter: smithay::wayland::viewporter::ViewporterState,
    /// The pointer image clients asked for, drawn on the hardware.
    pub cursor: CursorImageStatus,
    /// The cursor the compositor shows while it moves or resizes a window,
    /// whatever the client under it asks for (ADR-0032).
    pub grab_cursor: Option<smithay::input::pointer::CursorIcon>,
    /// The pointer is on the line between tiled windows (ADR-0054).
    pub split_hover: bool,
    /// A screenshot's flash, fading out over what was taken.
    pub flash: Option<crate::screenshot::Flash>,
    /// The Print key's choices, and a selection being made (ADR-0091).
    pub shots: crate::screenshot::Shots,
    /// Screen capture for other programs and the desktop's recording
    /// (ADR-0092).
    pub capture: crate::capture::Capture,
    /// What a client is dragging, drawn at the pointer.
    pub dnd_icon: Option<crate::dnd::Icon>,
    /// The window covering the screen, if one is (ADR-0041).
    pub fullscreen: Option<Window>,
    /// Windows out of the space, minimised to their tabs (ADR-0084).
    pub minimized: crate::minimize::Minimized,
    /// Tiling by dragging, and the tiler deciding it (ADR-0049).
    pub tiling: crate::tiling::Tiling,
    _cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState,
    /// Mouse-look for games (constraints.rs).
    _relative_pointer: smithay::wayland::relative_pointer::RelativePointerManagerState,
    _pointer_constraints: smithay::wayland::pointer_constraints::PointerConstraintsState,
    /// Or the hardware itself (ADR-0016).
    pub udev: Option<crate::udev::UdevData>,
    /// The window manager for X11 apps, once Xwayland is ready (ADR-0040).
    pub xwm: Option<smithay::xwayland::X11Wm>,
    pub xwayland_shell: smithay::wayland::xwayland_shell::XWaylandShellState,
}

impl State {
    pub fn new(
        event_loop: &mut EventLoop<'static, State>,
        display: Display<State>,
        output: Output,
        nested: Option<Nested>,
        winit: Option<crate::winit::WinitData>,
        config: crate::config::Config,
        auto_scale: f64,
    ) -> Result<Self, String> {
        let dh = display.handle();
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        // The system's layout; if xkb rejects it, the US default rather than
        // no keyboard at all.
        let keyboard = crate::keyboard::KeyboardSettings::load(&config.keyboard);
        let (rate, delay) = (config.keyboard.repeat_rate, config.keyboard.repeat_delay);
        if let Err(e) = seat.add_keyboard(keyboard.config(), delay, rate) {
            eprintln!(
                "patina-compositor: keyboard layout {:?}: {e}; using US",
                keyboard.layout
            );
            seat.add_keyboard(Default::default(), delay, rate)
                .map_err(|e| format!("keyboard: {e}"))?;
        }
        seat.add_pointer();
        // Whether or not there's a touchscreen: apps only look for touch
        // where the seat has it (ADR-0094).
        seat.add_touch();
        let output_global = output.create_global::<State>(&dh);
        crate::shadow::init(&dh);
        crate::shadow::set_enabled(config.shadows.enabled);
        let mut space = Space::default();
        space.map_output(&output, (0, 0));

        let socket =
            ListeningSocketSource::new_auto().map_err(|e| format!("no Wayland socket: {e}"))?;
        let socket_name = socket.socket_name().to_os_string();
        let handle = event_loop.handle();
        handle
            .insert_source(socket, |stream, _, state| {
                if let Err(e) = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    eprintln!("patina-compositor: client refused: {e}");
                }
            })
            .map_err(|e| e.to_string())?;
        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: the display is never dropped while the loop runs.
                    unsafe {
                        display
                            .get_mut()
                            .dispatch_clients(state)
                            .map_err(std::io::Error::other)?
                    };
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| e.to_string())?;

        Ok(Self {
            start: Instant::now(),
            socket_name,
            loop_signal: event_loop.get_signal(),
            loop_handle: event_loop.handle(),
            space,
            popups: PopupManager::default(),
            outputs: vec![output.clone()],
            output_globals: vec![(output.clone(), output_global)],
            output,
            needs_redraw: true,
            restart: false,
            placed: 0,
            compositor_state: CompositorState::new::<Self>(&dh),
            // Only what the compositor does, so clients hide the rest of
            // their window buttons.
            xdg_shell_state: XdgShellState::new_with_capabilities::<Self>(
                &dh,
                [
                    xdg_toplevel::WmCapabilities::Maximize,
                    xdg_toplevel::WmCapabilities::Minimize,
                ],
            ),
            _xdg_decoration_state:
                smithay::wayland::shell::xdg::decoration::XdgDecorationState::new::<Self>(&dh),
            layer_shell_state: WlrLayerShellState::new::<Self>(&dh),
            shm_state: ShmState::new::<Self>(&dh, vec![]),
            _output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(&dh),
            seat_state,
            data_device_state: DataDeviceState::new::<Self>(&dh),
            data_control_state:
                smithay::wayland::selection::wlr_data_control::DataControlState::new::<Self, _>(
                    &dh,
                    None,
                    |_| true,
                ),
            ext_data_control_state:
                smithay::wayland::selection::ext_data_control::DataControlState::new::<Self, _>(
                    &dh,
                    None,
                    |_| true,
                ),
            seat,
            toplevels: ForeignToplevels::new(&dh),
            night_light: crate::night_light::NightLight::new(&config.night_light),
            layouts: Default::default(),
            keys: crate::keys::Keys::new(&config.keys, &config.remap).0,
            config,
            lock: crate::lock::Lock::Unlocked,
            session_lock: smithay::wayland::session_lock::SessionLockManagerState::new::<Self, _>(
                &dh,
                |_| true,
            ),
            idle: Default::default(),
            _idle_inhibit: smithay::wayland::idle_inhibit::IdleInhibitManagerState::new::<Self>(
                &dh,
            ),
            lock_shown: Default::default(),
            super_tap: false,
            cmd: Default::default(),
            remapped: Default::default(),
            caps: Default::default(),
            frames: Default::default(),
            typing: Default::default(),
            clipboard: Default::default(),
            drag: Default::default(),
            auto_scale,
            _fractional_scale: smithay::wayland::fractional_scale::FractionalScaleManagerState::new::<
                Self,
            >(&dh),
            _viewporter: smithay::wayland::viewporter::ViewporterState::new::<Self>(&dh),
            cursor: CursorImageStatus::default_named(),
            grab_cursor: None,
            split_hover: false,
            flash: None,
            shots: Default::default(),
            capture: crate::capture::Capture::new(&dh),
            dnd_icon: None,
            fullscreen: None,
            minimized: Default::default(),
            tiling: Default::default(),
            _cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Self>(
                &dh,
            ),
            _relative_pointer: smithay::wayland::relative_pointer::RelativePointerManagerState::new::<
                Self,
            >(&dh),
            _pointer_constraints:
                smithay::wayland::pointer_constraints::PointerConstraintsState::new::<Self>(&dh),
            udev: None,
            xwm: None,
            xwayland_shell: smithay::wayland::xwayland_shell::XWaylandShellState::new::<Self>(&dh),
            display_handle: dh,
            nested,
            winit,
        })
    }

    /// The surface under `pos` and its origin: overlay and top layers, then
    /// windows, then the bottom and background layers.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        // Locked, there's only the locker's surface, covering the screen.
        if self.locked() {
            return self
                .lock_surface()
                .map(|s| (s.clone(), Point::from((0.0, 0.0))));
        }
        // A layer map is in its output's own coordinates.
        let output = self.output_under(pos);
        let shift = self
            .space
            .output_geometry(output)
            .map(|g| g.loc)
            .unwrap_or_default();
        let map = layer_map_for_output(output);
        let layer_under = |layer: Layer| {
            let local = pos - shift.to_f64();
            let surface = map.layer_under(layer, local)?;
            let origin = map.layer_geometry(surface)?.loc;
            surface
                .surface_under(local - origin.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + origin + shift).to_f64()))
        };
        layer_under(Layer::Overlay)
            .or_else(|| self.fullscreen_under(pos))
            .or_else(|| layer_under(Layer::Top))
            .or_else(|| {
                self.space.element_under(pos).and_then(|(window, origin)| {
                    window
                        .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + origin).to_f64()))
                })
            })
            .or_else(|| layer_under(Layer::Bottom))
            .or_else(|| layer_under(Layer::Background))
    }

    pub fn pointer_motion(&mut self, pos: Point<f64, Logical>, time: u32) {
        // On the hardware the compositor draws the pointer itself.
        self.needs_redraw = true;
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        // Selecting a part of the screen to take, no window has the pointer.
        if self.select_moved(pos) {
            pointer.motion(
                self,
                None,
                &MotionEvent {
                    location: pos,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
            return;
        }
        // On the line between tiled windows, neither window has the
        // pointer.
        // Nor on a window's frame (ADR-0077).
        let grabbed = pointer.is_grabbed();
        let under = if (!grabbed && self.frame_pointer(pos)) || self.hover_split(pos, grabbed) {
            None
        } else {
            self.surface_under(pos)
        };
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(self);
    }

    /// `button` is an evdev code, as both Wayland and Smithay use.
    pub fn pointer_button(&mut self, button: u32, pressed: bool, time: u32) {
        // A click while Super is held (Super+drag) isn't a tap.
        self.super_tap = false;
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        let serial = SERIAL_COUNTER.next_serial();
        // Locked, a click goes to the locker and changes nothing else.
        if self.locked() {
            let state = if pressed {
                smithay::backend::input::ButtonState::Pressed
            } else {
                smithay::backend::input::ButtonState::Released
            };
            pointer.button(
                self,
                &ButtonEvent {
                    button,
                    state,
                    serial,
                    time,
                },
            );
            pointer.frame(self);
            return;
        }
        // Selecting a part of the screen to take, clicks are the selection's.
        if self.select_button(button, pressed, pointer.current_location()) {
            return;
        }
        // A click on the line between tiled windows drags it (ADR-0054).
        if pressed
            && button == grabs::BTN_LEFT
            && !pointer.is_grabbed()
            && let Some(grab) = self.start_split(GrabStartData {
                focus: None,
                button,
                location: pointer.current_location(),
            })
        {
            pointer.set_grab(self, grab, serial, Focus::Clear);
            return;
        }
        // A click on a window's frame is the frame's (ADR-0077).
        if !pointer.is_grabbed() && self.frame_button(button, pressed, pointer.current_location()) {
            return;
        }
        // A click on a layer surface (the bar, a launcher) focuses it only
        // if it takes the keyboard, and leaves the windows' focus alone.
        let on_layer = self
            .surface_under(pointer.current_location())
            .and_then(|(surface, _)| self.layer_for(&surface));
        if pressed && let Some(layer) = on_layer {
            if layer.can_receive_keyboard_focus() {
                self.focus_layer(&layer, serial);
            }
        } else if pressed && !pointer.is_grabbed() {
            let under = self
                .space
                .element_under(pointer.current_location())
                .map(|(w, l)| (w.clone(), l));
            self.focus(under.as_ref().map(|(w, _)| w.clone()), serial);
            // Super+drag moves any window, whatever it's showing.
            let logo = self
                .seat
                .get_keyboard()
                .is_some_and(|k| k.modifier_state().logo);
            // A right click on the desktop opens the desktop's menu there
            // (ADR-0029).
            if under.is_none() && button == grabs::BTN_RIGHT && !logo {
                self.open_desktop_menu(pointer.current_location());
            }
            if logo
                && button == grabs::BTN_LEFT
                && let Some((window, _)) = under
            {
                let start = GrabStartData {
                    focus: None,
                    button,
                    location: pointer.current_location(),
                };
                self.start_move(window, start, pointer, serial);
                return;
            }
        }
        let state = if pressed {
            smithay::backend::input::ButtonState::Pressed
        } else {
            smithay::backend::input::ButtonState::Released
        };
        pointer.button(
            self,
            &ButtonEvent {
                button,
                state,
                serial,
                time,
            },
        );
        pointer.frame(self);
    }

    /// Runs `desktop_menu` with the pointer's position, in logical pixels.
    fn open_desktop_menu(&self, at: Point<f64, Logical>) {
        let command = &self.config.desktop_menu;
        if !command.is_empty() {
            self.spawn_client(&format!("{command} --at {:.0},{:.0}", at.x, at.y));
        }
    }

    pub fn pointer_axis(&mut self, frame: AxisFrame) {
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// Gives `window` the keyboard and raises it; `None` clears focus.
    pub(crate) fn layer_for(&self, surface: &WlSurface) -> Option<LayerSurface> {
        self.outputs.iter().find_map(|output| {
            layer_map_for_output(output)
                .layer_for_surface(surface, WindowSurfaceType::ALL)
                .cloned()
        })
    }

    /// Adds the monitors after the primary (ADR-0121), each to the right of
    /// the one before until it's placed otherwise.
    pub fn add_outputs(&mut self, extra: Vec<Output>) {
        let mut all = vec![self.output.clone()];
        all.extend(extra);
        self.outputs_changed(all, &[]);
    }

    /// The monitors are now `all`, the primary first, after some came and
    /// `removed` went: advertises the new ones, places them all, and moves
    /// the windows (those on a removed monitor go to the primary, those on
    /// one that shifted go with it).
    pub fn outputs_changed(&mut self, all: Vec<Output>, removed: &[Output]) {
        use crate::layout::{Entry, rects};
        let old: Vec<(Output, smithay::utils::Rectangle<i32, Logical>)> = self
            .outputs
            .iter()
            .filter_map(|o| Some((o.clone(), self.space.output_geometry(o)?)))
            .collect();
        let homes: Vec<(Window, Output)> = self
            .space
            .elements()
            .filter_map(|w| {
                let g = self.space.element_geometry(w)?;
                let middle = g.loc.to_f64() + g.size.to_f64().downscale(2.0).to_point();
                let (home, _) = old.iter().find(|(_, r)| r.to_f64().contains(middle))?;
                Some((w.clone(), home.clone()))
            })
            .collect();

        for output in removed {
            // Its bar and wallpaper go with it.
            for layer in layer_map_for_output(output).layers() {
                layer.layer_surface().send_close();
            }
            self.space.unmap_output(output);
            if let Some(i) = self.output_globals.iter().position(|(o, _)| o == output) {
                let (_, id) = self.output_globals.remove(i);
                self.display_handle.remove_global::<State>(id);
            }
        }
        for output in &all {
            if !self.output_globals.iter().any(|(o, _)| o == output) {
                let id = output.create_global::<State>(&self.display_handle);
                self.output_globals.push((output.clone(), id));
            }
        }

        let entries: Vec<Entry> = all
            .iter()
            .map(|o| Entry {
                id: o.name(),
                size: o
                    .current_mode()
                    .map(|m| {
                        m.size
                            .to_f64()
                            .to_logical(o.current_scale().fractional_scale())
                            .to_i32_round()
                    })
                    .unwrap_or_default(),
                place: None,
            })
            .collect();
        for (output, rect) in all.iter().zip(rects(&entries)) {
            self.space.map_output(output, rect.loc);
            output.change_current_state(None, None, None, Some(rect.loc));
            eprintln!(
                "patina-compositor: {} at {},{} ({}x{} points)",
                output.name(),
                rect.loc.x,
                rect.loc.y,
                rect.size.w,
                rect.size.h
            );
        }
        self.output = all[0].clone();
        self.outputs = all;
        for output in &self.outputs {
            layer_map_for_output(output).arrange();
        }

        let fullscreen_lost = self.fullscreen.clone().filter(|f| {
            homes
                .iter()
                .any(|(w, home)| w == f && removed.contains(home))
        });

        // Windows follow their monitor, or leave a removed one for the
        // primary, kept inside its work area.
        let primary = self.work_area_on(&self.output.clone());
        for (window, home) in homes {
            let Some(at) = self.space.element_location(&window) else {
                continue;
            };
            let old_home = old.iter().find(|(o, _)| *o == home).map(|(_, r)| r.loc);
            let new_home = self.space.output_geometry(&home).map(|r| r.loc);
            let to = match (old_home, new_home) {
                (Some(was), Some(now)) => at + (now - was),
                (Some(was), None) => {
                    let size = window.geometry().size;
                    let offset = at - was;
                    let x = offset.x.clamp(0, (primary.size.w - size.w).max(0));
                    let y = offset.y.clamp(0, (primary.size.h - size.h).max(0));
                    primary.loc + Point::from((x, y))
                }
                _ => continue,
            };
            self.place(&window, to);
        }
        if let Some(window) = fullscreen_lost {
            self.unfullscreen(&window);
        }
        self.relayout_maximized();
        self.update_preferred_scale();
        // The pointer may have been on a monitor that's gone.
        let pointer = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        let monitors: Vec<_> = self
            .outputs
            .iter()
            .filter_map(|o| self.space.output_geometry(o))
            .collect();
        let inside = crate::layout::clamp(&monitors, pointer);
        if inside != pointer {
            self.pointer_motion(inside, 0);
        }
        self.needs_redraw = true;
    }

    /// What `output` shows.
    pub fn view_of(&self, output: &Output) -> crate::layout::View {
        crate::layout::View {
            area: self.space.output_geometry(output).unwrap_or_default(),
            scale: output.current_scale().fractional_scale(),
        }
    }

    /// The primary's view, for the backends that have one monitor.
    pub fn primary_view(&self) -> crate::layout::View {
        self.view_of(&self.output)
    }

    /// The monitor under `pos`, else the primary.
    pub fn output_under(&self, pos: Point<f64, Logical>) -> &Output {
        self.outputs
            .iter()
            .find(|o| {
                self.space
                    .output_geometry(o)
                    .is_some_and(|g| g.to_f64().contains(pos))
            })
            .unwrap_or(&self.output)
    }

    /// The monitor `window`'s middle is on, else the pointer's.
    pub fn output_of(&self, window: &Window) -> Output {
        self.space
            .element_geometry(window)
            .map(|g| self.output_under(g.loc.to_f64() + g.size.to_f64().downscale(2.0).to_point()))
            .unwrap_or_else(|| self.pointer_output())
            .clone()
    }

    /// The monitor the pointer is on.
    pub fn pointer_output(&self) -> &Output {
        let pos = self
            .seat
            .get_pointer()
            .map(|p| p.current_location())
            .unwrap_or_default();
        self.output_under(pos)
    }

    /// Whether `pos` is under a menu or the bar: an overlay or top layer.
    pub fn over_layer(&self, pos: Point<f64, Logical>) -> bool {
        use smithay::wayland::shell::wlr_layer::Layer;
        let output = self.output_under(pos);
        let Some(geo) = self.space.output_geometry(output) else {
            return false;
        };
        let local = pos - geo.loc.to_f64();
        let map = layer_map_for_output(output);
        map.layer_under(Layer::Overlay, local).is_some()
            || map.layer_under(Layer::Top, local).is_some()
    }

    /// Gives the keyboard back to the window on top, as when a launcher
    /// that had it closes.
    pub(crate) fn focus_top_window(&mut self) {
        let top = self.space.elements().next_back().cloned();
        eprintln!(
            "patina-compositor: focusing the top window, {}",
            top.as_ref()
                .and_then(|w| w.toplevel().map(|t| crate::toplevels::name(t.wl_surface())))
                .unwrap_or_else(|| "none".into())
        );
        self.focus(top, SERIAL_COUNTER.next_serial());
    }

    /// Focuses and raises the window of `surface`, as a click on its tab in
    /// the bar does.
    pub fn activate_toplevel(&mut self, surface: &WlSurface) {
        if let Some(window) = self.window_for(surface) {
            self.focus(Some(window), SERIAL_COUNTER.next_serial());
        }
    }

    /// Asks the window of `surface` to close, as its own close button
    /// would.
    pub fn close_toplevel(&mut self, surface: &WlSurface) {
        let Some(window) = self.window_for(surface) else {
            return;
        };
        if let Some(toplevel) = window.toplevel() {
            toplevel.send_close();
        } else if let Some(x11) = window.x11_surface() {
            let _ = x11.close();
        }
    }

    /// Moves a mapped window without changing the stacking order or which
    /// window is active; only focusing a window raises it. (Smithay's
    /// `map_element` puts the window on top, so the ones that were above
    /// it are raised back over it, in order.)
    pub fn place(&mut self, window: &Window, location: Point<i32, Logical>) {
        // A minimised window stays off screen, whatever grab still has it.
        if self.space.element_location(window) == Some(location) || self.is_minimized(window) {
            return;
        }
        let above: Vec<Window> = self
            .space
            .elements()
            .skip_while(|w| *w != window)
            .skip(1)
            .cloned()
            .collect();
        self.space.map_element(window.clone(), location, false);
        for w in &above {
            self.space.raise_element(w, false);
        }
        self.tell_x11_where(window, location);
        self.needs_redraw = true;
    }

    /// Where the next new window goes: down and right of the last, in the
    /// area the bar leaves free.
    pub fn cascade(&mut self) -> Point<i32, Logical> {
        let output = self.pointer_output();
        let mut zone = layer_map_for_output(output).non_exclusive_zone();
        if let Some(geo) = self.space.output_geometry(output) {
            zone.loc += geo.loc;
        }
        let step = CASCADE_START + CASCADE_STEP * (self.placed % 8);
        self.placed += 1;
        (zone.loc.x + step, zone.loc.y + step).into()
    }

    /// Focuses and raises `window`.
    pub fn focus_window(&mut self, window: Window) {
        self.focus(Some(window), SERIAL_COUNTER.next_serial());
    }

    /// Puts the pointer in the middle of the screen, where it starts on the
    /// hardware.
    pub fn centre_pointer(&mut self) {
        if let Some(screen) = self.space.output_geometry(&self.output) {
            let centre = screen.loc.to_f64() + screen.size.to_f64().downscale(2.0).to_point();
            self.pointer_motion(centre, 0);
        }
    }

    /// Lets clients use every shm format the renderer can draw.
    pub fn set_shm_formats(
        &mut self,
        formats: impl IntoIterator<Item = smithay::reexports::wayland_server::protocol::wl_shm::Format>,
    ) {
        self.shm_state.update_formats(formats);
    }

    /// Gives the keyboard to a layer surface (a launcher), leaving every
    /// window looking inactive while it has it.
    pub(crate) fn focus_layer(&mut self, layer: &LayerSurface, serial: Serial) {
        // Locked, only the locker has the keyboard.
        if self.locked() {
            return;
        }
        self.activate(None);
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        keyboard.set_focus(self, Some(layer.wl_surface().clone().into()), serial);
        self.needs_redraw = true;
    }

    /// Marks `window` as the active one, and every other as inactive.
    pub(crate) fn activate(&self, window: Option<&Window>) {
        for w in self.space.elements() {
            w.set_activated(Some(w) == window);
            if let Some(t) = w.toplevel() {
                t.send_pending_configure();
            }
        }
    }

    pub(crate) fn focus(&mut self, window: Option<Window>, serial: Serial) {
        // Locked, only the locker has the keyboard; a window opening or
        // closing behind it changes nothing.
        if self.locked() {
            return;
        }
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        // Where the keyboard came from, so closing this window gives it back
        // there (ADR-0059).
        if let (Some(window), Some(from)) = (&window, self.focused_window())
            && from != *window
        {
            crate::tabs::set_opener(window, &from);
        }
        if let Some(window) = &window {
            self.unminimize(window);
            self.space.raise_element(window, true);
            self.raise_x11(window);
            self.restore_x11(window);
        }
        self.activate(window.as_ref());
        let target = window.and_then(|w| match w.x11_surface() {
            Some(x) => Some(crate::focus::FocusTarget::X11(x.clone())),
            None => w.toplevel().map(|t| t.wl_surface().clone().into()),
        });
        keyboard.set_focus(self, target, serial);
        self.needs_redraw = true;
    }

    /// For a client's move or resize request: the grab's start, if the
    /// pointer really is pressed on that client (so a client can't start a
    /// grab on its own), with the window and the pointer.
    #[allow(clippy::type_complexity)]
    fn client_grab(
        &self,
        surface: &ToplevelSurface,
        seat: &wl_seat::WlSeat,
        serial: Serial,
    ) -> Option<(
        GrabStartData<State>,
        Window,
        smithay::input::pointer::PointerHandle<State>,
    )> {
        let seat = Seat::<State>::from_resource(seat)?;
        let pointer = seat.get_pointer()?;
        if !pointer.has_grab(serial) {
            return None;
        }
        let start = pointer.grab_start_data()?;
        let (focus, _) = start.focus.as_ref()?;
        if !focus.id().same_client_as(&surface.wl_surface().id()) {
            return None;
        }
        let window = self.window_for(surface.wl_surface())?;
        Some((start, window, pointer))
    }

    /// Asks every window of the focused window's app (its Wayland client)
    /// to close. There's no "quit" request in Wayland; apps quit when their
    /// last window closes, and each window can still ask about unsaved work.
    pub fn quit_focused_app(&mut self) {
        let Some(focused) = self.focused_window() else {
            return;
        };
        // Every X app is the one Xwayland client: its windows go by the
        // app's process instead.
        if let Some(pid) = focused.x11_surface().and_then(|x| x.pid()) {
            for window in self.space.elements().chain(self.minimized.windows()) {
                if let Some(x11) = window.x11_surface().filter(|x| x.pid() == Some(pid)) {
                    let _ = x11.close();
                }
            }
            return;
        }
        let Some(client) = focused.toplevel().and_then(|t| t.wl_surface().client()) else {
            return;
        };
        let windows: Vec<ToplevelSurface> = self
            .space
            .elements()
            .chain(self.minimized.windows())
            .filter_map(|w| w.toplevel().cloned())
            .filter(|t| t.wl_surface().client().as_ref() == Some(&client))
            .collect();
        for toplevel in windows {
            toplevel.send_close();
        }
    }

    /// The window with the keyboard, if a window has it.
    pub fn focused_window(&self) -> Option<Window> {
        use smithay::wayland::seat::WaylandFocus;
        let focus = self.seat.get_keyboard()?.current_focus()?;
        let surface = focus.wl_surface()?.into_owned();
        self.window_for(&surface)
    }

    /// The window drawn on `surface`, Wayland or X11, shown or minimised.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        use smithay::wayland::seat::WaylandFocus;
        self.space
            .elements()
            .chain(self.minimized.windows())
            .find(|w| w.wl_surface().as_deref() == Some(surface))
            .cloned()
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(window) = self.window_for(&root) else {
            return;
        };
        let (Some(output_geo), Some(window_geo)) = (
            self.space
                .element_geometry(&window)
                .and_then(|g| self.space.output_geometry(self.output_under(g.loc.to_f64()))),
            self.space.element_geometry(&window),
        ) else {
            return;
        };
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        target.loc -= window_geo.loc;
        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }
}

#[derive(Default)]
pub struct ClientState {
    compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _: ClientId) {}
    fn disconnected(&self, _: ClientId, _: DisconnectReason) {}
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Xwayland's connection is made by Smithay, with its own data.
        if let Some(xwayland) = client.get_data::<smithay::xwayland::XWaylandClientData>() {
            return &xwayland.compositor_state;
        }
        &client
            .get_data::<ClientState>()
            .expect("every client gets ClientState")
            .compositor_state
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        crate::sync::wait_for_buffers(surface);
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.needs_redraw = true;
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for(&root) {
                window.on_commit();
                self.window_shown(&window);
            }
        }

        // A new toplevel gets its first configure on its first commit.
        if let Some(window) = self.window_for(surface) {
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .is_some_and(|d| d.lock().is_ok_and(|d| d.initial_configure_sent))
            });
            if !sent && let Some(t) = window.toplevel() {
                t.send_configure();
            }
        }

        grabs::handle_commit(self, surface);
        self.dnd_icon_commit(surface);
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface)
            && !popup.is_initial_configure_sent()
        {
            let _ = popup.send_configure();
        }

        // Layer surfaces: arrange, which may resize them, then send the
        // first configure.
        let layer_output = self
            .outputs
            .iter()
            .find(|o| {
                layer_map_for_output(o)
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .cloned();
        if let Some(output) = layer_output {
            let mut map = layer_map_for_output(&output);
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .is_some_and(|d| d.lock().is_ok_and(|d| d.initial_configure_sent))
            });
            map.arrange();
            let layer = map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .cloned();
            drop(map);
            self.relayout_maximized();
            if let Some(layer) = layer {
                if !sent {
                    layer.layer_surface().send_configure();
                } else if layer.cached_state().keyboard_interactivity
                    == KeyboardInteractivity::Exclusive
                {
                    // A launcher or lock screen asking for every key gets
                    // them once it's configured.
                    let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
                    if !keyboard
                        .current_focus()
                        .is_some_and(|f| f.is(layer.wl_surface()))
                    {
                        self.focus_layer(&layer, SERIAL_COUNTER.next_serial());
                    }
                }
            }
        }
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface);
        let location = self.cascade();
        self.space.map_element(window.clone(), location, false);
        self.focus(Some(window), SERIAL_COUNTER.next_serial());
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.fullscreen(&window);
        } else {
            // Not shown yet: it opens covering the screen.
            let size = self
                .space
                .output_geometry(self.pointer_output())
                .unwrap_or_else(|| self.screen())
                .size;
            surface.with_pending_state(|s| {
                s.states
                    .set(smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel::State::Fullscreen);
                s.size = Some(size);
            });
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.unfullscreen(&window);
        }
    }

    fn toplevel_destroyed(&mut self, toplevel: ToplevelSurface) {
        let window = self.window_for(toplevel.wl_surface());
        if let Some(window) = &window {
            self.window_closed(window);
        }
        self.focus_after_close(toplevel.wl_surface());
        if let Some(window) = &window {
            self.forget_minimized(window);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _: PositionerState) {
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _: PopupSurface, _: wl_seat::WlSeat, _: Serial) {}

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some((start, window, pointer)) = self.client_grab(&surface, &seat, serial) else {
            return;
        };
        self.start_move(window, start, pointer, serial);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.maximize(&window);
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.unmaximize(&window, None);
        }
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for(surface.wl_surface()) {
            self.minimize(&window);
        }
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some((start, window, pointer)) = self.client_grab(&surface, &seat, serial) else {
            return;
        };
        let Some(location) = self.space.element_location(&window) else {
            return;
        };
        let initial = smithay::utils::Rectangle::new(location, window.geometry().size);
        let edges: grabs::Edges = edges.into();
        if let Some(grab) = ResizeGrab::start(start, window, edges, initial) {
            self.grab_cursor = Some(edges.cursor());
            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }
}

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wanted: Option<wl_output::WlOutput>,
        _: Layer,
        namespace: String,
    ) {
        // The output it asked for (the bar and wallpaper ask for each), else
        // the one the pointer is on.
        let output = wanted
            .as_ref()
            .and_then(Output::from_resource)
            .unwrap_or_else(|| self.pointer_output().clone());
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            eprintln!("patina-compositor: layer surface refused: {e}");
        }
        drop(map);
        // Its monitor's scale, which may not be the primary's.
        self.update_preferred_scale();
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        for output in &self.outputs {
            let mut map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = &layer {
                map.unmap_layer(layer);
                break;
            }
        }
        self.relayout_maximized();
        // Its client may be gone already, taking the focus with it, so
        // also refocus when the keyboard is on nothing or a dead surface.
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let stranded = keyboard
            .current_focus()
            .is_none_or(|f| !smithay::utils::IsAlive::alive(&f) || f.is(surface.wl_surface()));
        if stranded {
            self.focus_top_window();
        }
        self.needs_redraw = true;
    }
}

impl SeatHandler for State {
    type KeyboardFocus = crate::focus::FocusTarget;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _: &Seat<Self>, image: CursorImageStatus) {
        self.cursor = image;
        self.needs_redraw = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&crate::focus::FocusTarget>) {
        use smithay::wayland::seat::WaylandFocus;
        let client = focused
            .and_then(|f| f.wl_surface())
            .and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client);
    }
}

impl SelectionHandler for State {
    /// What the compositor puts on the clipboard: typed text, or the
    /// clipboard given back after it (ADR-0071).
    type SelectionUserData = crate::typing::Served;

    fn new_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        source: Option<smithay::wayland::selection::SelectionSource>,
        _seat: Seat<Self>,
    ) {
        if ty == smithay::wayland::selection::SelectionTarget::Clipboard {
            let types = source.map(|s| s.mime_types());
            self.clipboard_offered(types.clone().unwrap_or_default());
            self.clipboard_selected(types);
        }
    }

    fn send_selection(
        &mut self,
        ty: smithay::wayland::selection::SelectionTarget,
        mime_type: String,
        fd: std::os::fd::OwnedFd,
        _seat: Seat<Self>,
        _served: &crate::typing::Served,
    ) {
        self.send_served(ty, &mime_type, fd);
    }
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }

    /// A drop copies with Ctrl held and moves with Shift, as on GNOME;
    /// otherwise the target's preference stands.
    fn action_choice(
        &mut self,
        available: smithay::reexports::wayland_server::protocol::wl_data_device_manager::DndAction,
        preferred: smithay::reexports::wayland_server::protocol::wl_data_device_manager::DndAction,
    ) -> smithay::reexports::wayland_server::protocol::wl_data_device_manager::DndAction {
        use smithay::reexports::wayland_server::protocol::wl_data_device_manager::DndAction;
        let mods = self
            .seat
            .get_keyboard()
            .map(|k| k.modifier_state())
            .unwrap_or_default();
        if mods.ctrl && available.contains(DndAction::Copy) {
            DndAction::Copy
        } else if mods.shift && available.contains(DndAction::Move) {
            DndAction::Move
        } else {
            smithay::wayland::selection::data_device::default_action_chooser(available, preferred)
        }
    }
}

impl smithay::wayland::selection::wlr_data_control::DataControlHandler for State {
    fn data_control_state(
        &self,
    ) -> &smithay::wayland::selection::wlr_data_control::DataControlState {
        &self.data_control_state
    }
}

impl smithay::wayland::selection::ext_data_control::DataControlHandler for State {
    fn data_control_state(
        &self,
    ) -> &smithay::wayland::selection::ext_data_control::DataControlState {
        &self.ext_data_control_state
    }
}

impl ServerDndGrabHandler for State {}
impl OutputHandler for State {}

delegate_compositor!(State);
delegate_shm!(State);
delegate_xdg_shell!(State);
smithay::delegate_xdg_decoration!(State);
delegate_layer_shell!(State);
delegate_seat!(State);
delegate_data_device!(State);
smithay::delegate_data_control!(State);
smithay::delegate_ext_data_control!(State);
delegate_output!(State);
smithay::delegate_cursor_shape!(State);

/// Needed by cursor-shape, which also serves tablet tools; there are none.
impl smithay::wayland::tablet_manager::TabletSeatHandler for State {}

impl State {
    /// A move or resize ended: the client under the pointer's cursor again.
    pub fn end_grab_cursor(&mut self) {
        self.grab_cursor = None;
        self.needs_redraw = true;
    }
}
