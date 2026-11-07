//! Runs the compositor as a window on another Wayland desktop (GNOME, or
//! the test VM's dwl): the output is drawn in software with pixman, copied
//! into a wl_shm buffer, and the window's pointer and keyboard drive the
//! compositor's seat (ADR-0008).
//!
//! No GPU, EGL or winit is involved, so it runs anywhere wl_shm does.

use std::time::Duration;

use smithay::{
    backend::{
        input::{Axis, AxisSource},
        renderer::{Bind, damage::OutputDamageTracker, pixman::PixmanRenderer},
    },
    desktop::layer_map_for_output,
    input::pointer::AxisFrame,
    output::{Mode, Output},
    reexports::pixman::{FormatCode, Image},
};
use smithay_client_toolkit::{
    compositor::{
        CompositorHandler as ClientCompositorHandler, CompositorState as ClientCompositorState,
        FrameCallbackData,
    },
    delegate_registry,
    output::{OutputHandler as ClientOutputHandler, OutputState as ClientOutputState},
    reexports::{
        calloop::EventLoop,
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        },
        protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
            Shape, WpCursorShapeDeviceV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler as ClientSeatHandler, SeatState as ClientSeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler as ClientShmHandler, slot::SlotPool},
};

use crate::state::State;

/// The window's size until the parent desktop says otherwise.
pub const DEFAULT_SIZE: (u32, u32) = (1280, 800);

/// The desktop behind every window, until there's a wallpaper.
pub const BACKGROUND: [f32; 4] = [0.13, 0.13, 0.14, 1.0];

pub struct Nested {
    qh: QueueHandle<State>,
    registry_state: RegistryState,
    seat_state: ClientSeatState,
    output_state: ClientOutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    cursor_shape: Option<CursorShapeManager>,
    pointer: Option<(wl_pointer::WlPointer, Option<WpCursorShapeDeviceV1>)>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    size: (u32, u32),
    configured: bool,
    frame_pending: bool,

    pub(crate) renderer: PixmanRenderer,
    image: Option<Image<'static, 'static>>,
    damage_tracker: OutputDamageTracker,
    /// How many frames old the image's contents are: 0 after a resize.
    age: usize,
}

impl Nested {
    /// Opens the window on the desktop named by `WAYLAND_DISPLAY`, and adds
    /// that connection to the event loop.
    pub fn connect(
        event_loop: &EventLoop<'static, State>,
        output: &Output,
    ) -> Result<Self, String> {
        let conn = Connection::connect_to_env()
            .map_err(|e| format!("no desktop to open a window on: {e}"))?;
        let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        WaylandSource::new(conn, queue)
            .insert(event_loop.handle())
            .map_err(|e| e.to_string())?;

        let compositor = ClientCompositorState::bind(&globals, &qh)
            .map_err(|e| format!("wl_compositor: {e}"))?;
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|e| format!("xdg_wm_base: {e}"))?;
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let pool = SlotPool::new((DEFAULT_SIZE.0 * DEFAULT_SIZE.1 * 4) as usize, &shm)
            .map_err(|e| e.to_string())?;

        let surface = compositor.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
        window.set_title("Patina");
        window.set_app_id("com.sectorflabs.PatinaCompositor");
        window.set_min_size(Some((320, 240)));
        window.commit();

        Ok(Self {
            registry_state: RegistryState::new(&globals),
            seat_state: ClientSeatState::new(&globals, &qh),
            output_state: ClientOutputState::new(&globals, &qh),
            cursor_shape: CursorShapeManager::bind(&globals, &qh).ok(),
            shm,
            pool,
            window,
            pointer: None,
            keyboard: None,
            size: DEFAULT_SIZE,
            configured: false,
            frame_pending: false,
            renderer: PixmanRenderer::new().map_err(|e| format!("pixman: {e}"))?,
            image: None,
            damage_tracker: OutputDamageTracker::from_output(output),
            age: 0,
            qh,
        })
    }
}

/// The output's mode for a window of `size`.
pub fn mode(size: (u32, u32)) -> Mode {
    Mode {
        size: (size.0 as i32, size.1 as i32).into(),
        refresh: 60_000,
    }
}

/// Draws the output into the window, if anything changed and the parent
/// desktop is ready for a frame, then tells clients to draw their next.
pub fn render(state: &mut State) {
    let locked = state.locked();
    let lock_surface = state.lock_surface().cloned();
    let scale = state.output.current_scale().fractional_scale();
    let ready = state
        .nested
        .as_ref()
        .is_some_and(|n| n.configured && !n.frame_pending);
    if !ready || !state.needs_redraw {
        return;
    }
    state.needs_redraw = false;
    // A screenshot's flash, where a drop would tile (ADR-0049), and a
    // selection being made (ADR-0091).
    let view = state.primary_view();
    let flash: Vec<_> = state
        .flash_element(view)
        .into_iter()
        .chain(state.preview_element(view))
        .chain(state.selection_elements(view))
        .collect();
    let dnd_icon = state.dnd_icon_at();
    let fullscreen = state.fullscreen_drawn();
    state.paint_frames();
    let fullscreen_window = state.fullscreen.clone();
    let Some(n) = state.nested.as_mut() else {
        return;
    };

    let (w, h) = (n.size.0 as usize, n.size.1 as usize);
    if n.image.as_ref().map(|i| (i.width(), i.height())) != Some((w, h)) {
        match Image::new(FormatCode::A8R8G8B8, w, h, true) {
            Ok(image) => n.image = Some(image),
            Err(e) => {
                eprintln!("patina-compositor: no {w}x{h} image: {e:?}");
                return;
            }
        }
        n.age = 0;
    }
    let Some(image) = n.image.as_mut() else {
        return;
    };

    let overlay = crate::screenshot::overlay(
        &mut n.renderer,
        lock_surface.as_ref(),
        scale,
        flash,
        dnd_icon.as_ref(),
        fullscreen.as_ref(),
        // Locked, only the locker, over black.
        (!locked).then_some(&state.space),
        &state.output,
        fullscreen_window.as_ref(),
    );
    let damaged = {
        let mut target = match n.renderer.bind(image) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("patina-compositor: {e}");
                return;
            }
        };
        match n.damage_tracker.render_output(
            &mut n.renderer,
            &mut target,
            n.age,
            &overlay,
            BACKGROUND,
        ) {
            Ok(result) => result.damage.is_some(),
            Err(e) => {
                eprintln!("patina-compositor: render failed: {e:?}");
                return;
            }
        }
    };

    if damaged || n.age == 0 {
        present(n, w, h);
        n.age = 1;
    }

    let time = state.start.elapsed();
    let output = state.output.clone();
    for window in state.space.elements() {
        window.send_frame(&output, time, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
    for layer in layer_map_for_output(&output).layers() {
        layer.send_frame(&output, time, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
    crate::lock::send_frames(lock_surface.as_ref(), &output, time);
    crate::dnd::send_frames(dnd_icon.as_ref(), &output, time);
    state.lock_drawn();
    state.space.refresh();
    state.popups.cleanup();
}

/// Copies the image into a shm buffer and commits it to the window.
fn present(n: &mut Nested, w: usize, h: usize) {
    let Some(image) = n.image.as_ref() else {
        return;
    };
    let (buffer, canvas) =
        match n
            .pool
            .create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)
        {
            Ok(b) => b,
            Err(e) => {
                eprintln!("patina-compositor: no {w}x{h} buffer: {e}");
                return;
            }
        };
    let stride = image.stride();
    // SAFETY: the image owns `stride * h` bytes, and nothing writes to it
    // while this slice lives.
    let pixels = unsafe { std::slice::from_raw_parts(image.data() as *const u8, stride * h) };
    for (row, dst) in canvas.chunks_exact_mut(w * 4).enumerate().take(h) {
        dst.copy_from_slice(&pixels[row * stride..row * stride + w * 4]);
    }
    let surface = n.window.wl_surface();
    surface.damage_buffer(0, 0, w as i32, h as i32);
    surface.frame(&n.qh, FrameCallbackData(surface.clone()));
    if let Err(e) = buffer.attach_to(surface) {
        eprintln!("patina-compositor: {e}");
        return;
    }
    n.window.commit();
    n.frame_pending = true;
}

impl State {
    /// The software backend, for the sctk handlers: its events only arrive
    /// when it's the backend in use.
    fn nested_mut(&mut self) -> &mut Nested {
        self.nested
            .as_mut()
            .expect("sctk events only arrive with the software backend")
    }
}

impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.loop_signal.stop();
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let Some(n) = self.nested.as_mut() else {
            return;
        };
        let size = (
            configure.new_size.0.map_or(n.size.0, |w| w.get()),
            configure.new_size.1.map_or(n.size.1, |h| h.get()),
        );
        if size != n.size || !n.configured {
            n.size = size;
            let mode = mode(size);
            self.output
                .change_current_state(Some(mode), None, None, None);
            self.output.set_preferred(mode);
            layer_map_for_output(&self.output).arrange();
            n.configured = true;
            self.relayout_maximized();
        }
        if let Some(n) = self.nested.as_mut() {
            n.configured = true;
        }
        self.needs_redraw = true;
    }
}

impl ClientCompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if let Some(n) = self.nested.as_mut() {
            n.frame_pending = false;
        }
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl ClientOutputHandler for State {
    fn output_state(&mut self) -> &mut ClientOutputState {
        &mut self.nested_mut().output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ClientShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.nested_mut().shm
    }
}

impl ClientSeatHandler for State {
    fn seat_state(&mut self) -> &mut ClientSeatState {
        &mut self.nested_mut().seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let Some(n) = self.nested.as_mut() else {
            return;
        };
        match capability {
            Capability::Pointer if n.pointer.is_none() => {
                if let Ok(pointer) = n.seat_state.get_pointer(qh, &seat) {
                    let device = n
                        .cursor_shape
                        .as_ref()
                        .map(|m| m.get_shape_device(&pointer, qh));
                    n.pointer = Some((pointer, device));
                }
            }
            Capability::Keyboard if n.keyboard.is_none() => {
                n.keyboard = n.seat_state.get_keyboard(qh, &seat, None).ok();
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        let Some(n) = self.nested.as_mut() else {
            return;
        };
        match capability {
            Capability::Pointer => {
                if let Some((pointer, device)) = n.pointer.take() {
                    if let Some(device) = device {
                        device.destroy();
                    }
                    pointer.release();
                }
            }
            Capability::Keyboard => {
                if let Some(keyboard) = n.keyboard.take() {
                    keyboard.release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let Some(window) = self.nested.as_ref().map(|n| n.window.wl_surface().clone()) else {
            return;
        };
        for event in events {
            if event.surface != window {
                continue;
            }
            self.input_activity();
            let pos = event.position.into();
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    // The compositor's clients ask for their own cursors,
                    // but there's no cursor drawing yet: show the default.
                    if let Some((_, Some(device))) =
                        self.nested.as_ref().and_then(|n| n.pointer.as_ref())
                    {
                        device.set_shape(serial, Shape::Default);
                    }
                    self.pointer_motion(pos, 0);
                }
                PointerEventKind::Leave { .. } => {}
                PointerEventKind::Motion { time } => self.pointer_motion(pos, time),
                PointerEventKind::Press { time, button, .. } => {
                    self.pointer_button(button, true, time)
                }
                PointerEventKind::Release { time, button, .. } => {
                    self.pointer_button(button, false, time)
                }
                PointerEventKind::Axis {
                    time,
                    horizontal,
                    vertical,
                    source,
                } => {
                    use smithay_client_toolkit::reexports::client::protocol::wl_pointer::AxisSource as Wl;
                    let source = match source {
                        Some(Wl::Finger) => AxisSource::Finger,
                        Some(Wl::Continuous) => AxisSource::Continuous,
                        Some(Wl::WheelTilt) => AxisSource::WheelTilt,
                        _ => AxisSource::Wheel,
                    };
                    let mut frame = AxisFrame::new(time).source(source);
                    for (axis, scroll) in
                        [(Axis::Horizontal, horizontal), (Axis::Vertical, vertical)]
                    {
                        if scroll.absolute != 0.0 {
                            frame = frame.value(axis, scroll.absolute);
                        }
                        if scroll.value120 != 0 {
                            frame = frame.v120(axis, scroll.value120);
                        }
                        if scroll.stop {
                            frame = frame.stop(axis);
                        }
                    }
                    self.pointer_axis(frame);
                }
            }
        }
    }
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.input_activity();
        if self.key(event.raw_code, true, event.time) == Some(crate::input::KeyAction::Quit) {
            self.loop_signal.stop();
        }
    }

    // Clients repeat keys themselves, from the seat's repeat info.
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.input_activity();
        let _ = self.key(event.raw_code, false, event.time);
    }

    // The seat works out modifiers from the keys themselves.
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.nested_mut().registry_state
    }
    registry_handlers![ClientOutputState, ClientSeatState];
}

delegate_registry!(State);
smithay_client_toolkit::delegate_dispatch2!(State);
