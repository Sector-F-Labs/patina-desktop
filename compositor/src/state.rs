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
        keyboard::{FilterResult, Keycode},
        pointer::{AxisFrame, ButtonEvent, CursorImageStatus, Focus, GrabStartData, MotionEvent},
    },
    output::Output,
    reexports::{
        calloop::{EventLoop, Interest, LoopSignal, Mode, PostAction, generic::Generic},
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
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
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

use crate::grabs::{self, MoveGrab, ResizeGrab};
use crate::nested::Nested;

/// Where the first window goes, inside the free area, and how far each
/// next one steps down and right.
const CASCADE_START: i32 = 48;
const CASCADE_STEP: i32 = 32;

pub struct State {
    pub start: Instant,
    pub socket_name: OsString,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,

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
    pub nested: Nested,
}

impl State {
    pub fn new(
        event_loop: &mut EventLoop<'static, State>,
        display: Display<State>,
        output: Output,
        nested: Nested,
    ) -> Result<Self, String> {
        let dh = display.handle();
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        seat.add_keyboard(Default::default(), 200, 25)
            .map_err(|e| format!("keyboard: {e}"))?;
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
            space,
            popups: PopupManager::default(),
            output,
            needs_redraw: true,
            placed: 0,
            compositor_state: CompositorState::new::<Self>(&dh),
            xdg_shell_state: XdgShellState::new::<Self>(&dh),
            layer_shell_state: WlrLayerShellState::new::<Self>(&dh),
            shm_state: ShmState::new::<Self>(&dh, vec![]),
            _output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(&dh),
            seat_state,
            data_device_state: DataDeviceState::new::<Self>(&dh),
            seat,
            display_handle: dh,
            nested,
        })
    }

    /// The surface under `pos` and its origin: overlay and top layers, then
    /// windows, then the bottom and background layers.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
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
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        let serial = SERIAL_COUNTER.next_serial();
        if pressed && !pointer.is_grabbed() {
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
            if logo
                && button == grabs::BTN_LEFT
                && let Some((window, location)) = under
            {
                let start = GrabStartData {
                    focus: None,
                    button,
                    location: pointer.current_location(),
                };
                pointer.set_grab(
                    self,
                    MoveGrab {
                        start,
                        window,
                        initial: location,
                    },
                    serial,
                    Focus::Clear,
                );
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

    pub fn pointer_axis(&mut self, frame: AxisFrame) {
        let pointer = self.seat.get_pointer().expect("seat has a pointer");
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// `code` is an evdev keycode.
    pub fn key(&mut self, code: u32, pressed: bool, time: u32) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        let state = if pressed {
            smithay::backend::input::KeyState::Pressed
        } else {
            smithay::backend::input::KeyState::Released
        };
        keyboard.input::<(), _>(
            self,
            Keycode::new(code + 8),
            state,
            SERIAL_COUNTER.next_serial(),
            time,
            |_, _, _| FilterResult::Forward,
        );
    }

    /// Gives `window` the keyboard and raises it; `None` clears focus.
    fn focus(&mut self, window: Option<Window>, serial: Serial) {
        let keyboard = self.seat.get_keyboard().expect("seat has a keyboard");
        if let Some(window) = &window {
            self.space.raise_element(window, true);
        }
        for w in self.space.elements() {
            w.set_activated(Some(w) == window.as_ref());
            if let Some(t) = w.toplevel() {
                t.send_pending_configure();
            }
        }
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

    fn window_for(&self, surface: &WlSurface) -> Option<Window> {
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
            if !sent
                && let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            {
                layer.layer_surface().send_configure();
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

    fn toplevel_destroyed(&mut self, _: ToplevelSurface) {
        // Focus the window now on top, if any.
        let top = self.space.elements().next_back().cloned();
        self.focus(top, SERIAL_COUNTER.next_serial());
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
        let Some(initial) = self.space.element_location(&window) else {
            return;
        };
        pointer.set_grab(
            self,
            MoveGrab {
                start,
                window,
                initial,
            },
            serial,
            Focus::Clear,
        );
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
        if let Some(grab) = ResizeGrab::start(start, window, edges.into(), initial) {
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
        if let Some(layer) = layer {
            map.unmap_layer(&layer);
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

    fn cursor_image(&mut self, _: &Seat<Self>, _: CursorImageStatus) {}

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
