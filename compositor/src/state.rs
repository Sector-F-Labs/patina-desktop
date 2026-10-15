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
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
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
    /// Something changed on screen since the last frame.
    pub needs_redraw: bool,
    placed: i32,

    compositor_state: CompositorState,
    xdg_shell_state: XdgShellState,
    layer_shell_state: WlrLayerShellState,
    shm_state: ShmState,
    _output_manager_state: OutputManagerState,
    seat_state: SeatState<State>,
    data_device_state: DataDeviceState,
    pub seat: Seat<State>,

    /// The window on the parent desktop that this compositor draws into.
    pub nested: Option<Nested>,
    /// Or the window it draws into with OpenGL ES (ADR-0015).
    pub winit: Option<crate::winit::WinitData>,
    /// What the bar's window list has been told.
    pub toplevels: ForeignToplevels,
    /// The settings from config.toml in use (ADR-0019).
    pub config: crate::config::Config,
    /// The lock screen (ADR-0025).
    pub lock: crate::lock::Lock,
    pub session_lock: smithay::wayland::session_lock::SessionLockManagerState,
    /// Locked, and a frame has been drawn since: the desktop is hidden.
    pub lock_shown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Super is down, and nothing else has been pressed since.
    pub super_tap: bool,
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
    _cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState,
    /// Or the hardware itself (ADR-0016).
    pub udev: Option<crate::udev::UdevData>,
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
        output.create_global::<State>(&dh);
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
            output,
            needs_redraw: true,
            placed: 0,
            compositor_state: CompositorState::new::<Self>(&dh),
            // Only what the compositor does, so clients hide the rest of
            // their window buttons.
            xdg_shell_state: XdgShellState::new_with_capabilities::<Self>(
                &dh,
                [xdg_toplevel::WmCapabilities::Maximize],
            ),
            layer_shell_state: WlrLayerShellState::new::<Self>(&dh),
            shm_state: ShmState::new::<Self>(&dh, vec![]),
            _output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(&dh),
            seat_state,
            data_device_state: DataDeviceState::new::<Self>(&dh),
            seat,
            toplevels: ForeignToplevels::new(&dh),
            config,
            lock: crate::lock::Lock::Unlocked,
            session_lock: smithay::wayland::session_lock::SessionLockManagerState::new::<Self, _>(
                &dh,
                |_| true,
            ),
            lock_shown: Default::default(),
            super_tap: false,
            drag: Default::default(),
            auto_scale,
            _fractional_scale: smithay::wayland::fractional_scale::FractionalScaleManagerState::new::<
                Self,
            >(&dh),
            _viewporter: smithay::wayland::viewporter::ViewporterState::new::<Self>(&dh),
            cursor: CursorImageStatus::default_named(),
            grab_cursor: None,
            _cursor_shape: smithay::wayland::cursor_shape::CursorShapeManagerState::new::<Self>(
                &dh,
            ),
            udev: None,
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
        let map = layer_map_for_output(&self.output);
        let layer_under = |layer: Layer| {
            let surface = map.layer_under(layer, pos)?;
            let origin = map.layer_geometry(surface)?.loc;
            surface
                .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
                .map(|(s, p)| (s, (p + origin).to_f64()))
        };
        layer_under(Layer::Overlay)
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
        let under = self.surface_under(pos);
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
    fn layer_for(&self, surface: &WlSurface) -> Option<LayerSurface> {
        layer_map_for_output(&self.output)
            .layer_for_surface(surface, WindowSurfaceType::ALL)
            .cloned()
    }

    /// Gives the keyboard back to the window on top, as when a launcher
    /// that had it closes.
    pub(crate) fn focus_top_window(&mut self) {
        let top = self.space.elements().next_back().cloned();
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
        if let Some(toplevel) = self.window_for(surface).and_then(|w| w.toplevel().cloned()) {
            toplevel.send_close();
        }
    }

    /// Moves a mapped window without changing the stacking order or which
    /// window is active; only focusing a window raises it. (Smithay's
    /// `map_element` puts the window on top, so the ones that were above
    /// it are raised back over it, in order.)
    pub fn place(&mut self, window: &Window, location: Point<i32, Logical>) {
        if self.space.element_location(window) == Some(location) {
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
        self.needs_redraw = true;
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
    fn focus_layer(&mut self, layer: &LayerSurface, serial: Serial) {
        // Locked, only the locker has the keyboard.
        if self.locked() {
            return;
        }
        self.activate(None);
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        keyboard.set_focus(self, Some(layer.wl_surface().clone()), serial);
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

    fn focus(&mut self, window: Option<Window>, serial: Serial) {
        // Locked, only the locker has the keyboard; a window opening or
        // closing behind it changes nothing.
        if self.locked() {
            return;
        }
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        if let Some(window) = &window {
            self.space.raise_element(window, true);
        }
        self.activate(window.as_ref());
        let surface = window.and_then(|w| w.toplevel().map(|t| t.wl_surface().clone()));
        keyboard.set_focus(self, surface, serial);
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
        let Some(client) = focused.toplevel().and_then(|t| t.wl_surface().client()) else {
            return;
        };
        let windows: Vec<ToplevelSurface> = self
            .space
            .elements()
            .filter_map(|w| w.toplevel().cloned())
            .filter(|t| t.wl_surface().client().as_ref() == Some(&client))
            .collect();
        for toplevel in windows {
            toplevel.send_close();
        }
    }

    /// The window with the keyboard, if a window has it.
    pub fn focused_window(&self) -> Option<Window> {
        let focus = self.seat.get_keyboard()?.current_focus()?;
        self.window_for(&focus)
    }

    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| w.toplevel().is_some_and(|t| t.wl_surface() == surface))
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
            self.space.output_geometry(&self.output),
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
        &client
            .get_data::<ClientState>()
            .expect("every client gets ClientState")
            .compositor_state
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
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(popup)) = self.popups.find_popup(surface)
            && !popup.is_initial_configure_sent()
        {
            let _ = popup.send_configure();
        }

        // Layer surfaces: arrange, which may resize them, then send the
        // first configure.
        let mut map = layer_map_for_output(&self.output);
        if map
            .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            .is_some()
        {
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
                    if keyboard.current_focus().as_ref() != Some(layer.wl_surface()) {
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
        let zone = layer_map_for_output(&self.output).non_exclusive_zone();
        let step = CASCADE_START + CASCADE_STEP * (self.placed % 8);
        self.placed += 1;
        self.space.map_element(
            window.clone(),
            (zone.loc.x + step, zone.loc.y + step),
            false,
        );
        self.focus(Some(window), SERIAL_COUNTER.next_serial());
    }

    fn toplevel_destroyed(&mut self, toplevel: ToplevelSurface) {
        self.focus_after_close(toplevel.wl_surface());
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
        _: Option<wl_output::WlOutput>,
        _: Layer,
        namespace: String,
    ) {
        // There is one output, so every layer surface goes on it.
        let mut map = layer_map_for_output(&self.output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            eprintln!("patina-compositor: layer surface refused: {e}");
        }
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        let mut map = layer_map_for_output(&self.output);
        let layer = map
            .layers()
            .find(|l| l.layer_surface() == &surface)
            .cloned();
        if let Some(layer) = &layer {
            map.unmap_layer(layer);
        }
        drop(map);
        self.relayout_maximized();
        // Its client may be gone already, taking the focus with it, so
        // also refocus when the keyboard is on nothing or a dead surface.
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let stranded = keyboard
            .current_focus()
            .is_none_or(|f| !f.is_alive() || &f == surface.wl_surface());
        if stranded {
            self.focus_top_window();
        }
        self.needs_redraw = true;
    }
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<State> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _: &Seat<Self>, image: CursorImageStatus) {
        self.cursor = image;
        self.needs_redraw = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| self.display_handle.get_client(s.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client);
    }
}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}
impl OutputHandler for State {}

delegate_compositor!(State);
delegate_shm!(State);
delegate_xdg_shell!(State);
delegate_layer_shell!(State);
delegate_seat!(State);
delegate_data_device!(State);
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
