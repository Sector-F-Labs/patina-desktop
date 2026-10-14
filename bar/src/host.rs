//! A layer-shell host for egui: one wlr-layer-shell surface per output,
//! each with its own egui context, drawn in software into wl_shm buffers.
//!
//! eframe/winit only make xdg-shell windows, so desktop pieces such as bars,
//! launchers and notifications need a host of their own (ADR-0003). Nothing
//! here is specific to the bar: a [`Shell`] says what to draw, and the host
//! does surfaces, outputs, scale, pointer input and painting.

use std::time::{Duration, Instant};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::EventLoop,
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        },
        protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
            Shape, WpCursorShapeDeviceV1,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{
            PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};

use crate::raster::Painter;

/// What a host draws.
pub trait Shell: 'static {
    /// Called for each new egui context, before its first frame, e.g. to
    /// apply a skin.
    fn setup(&mut self, ctx: &egui::Context);
    /// Draws the surface on `output`, the compositor's name for it (the
    /// name dwl uses in its status, e.g. `HEADLESS-1`).
    fn ui(&mut self, ui: &mut egui::Ui, output: &str);
}

/// Where the host's surfaces go.
pub struct Options {
    /// The layer-shell namespace, which compositors use in their rules.
    pub namespace: String,
    pub layer: Layer,
    /// `TOP` or `BOTTOM`; the surface spans the output's width.
    pub edge: Anchor,
    /// Height in logical pixels, also reserved as the exclusive zone.
    pub height: u32,
}

struct Surface {
    output: wl_output::WlOutput,
    name: String,
    layer: LayerSurface,
    ctx: egui::Context,
    painter: Painter,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    scale: i32,
    configured: bool,
    dirty: bool,
    frame_pending: bool,
    repaint_at: Option<Instant>,
    events: Vec<egui::Event>,
}

pub struct Host<S: Shell> {
    pub shell: S,
    options: Options,
    qh: QueueHandle<Self>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,
    cursor_shape: Option<CursorShapeManager>,
    pointer: Option<(wl_pointer::WlPointer, Option<WpCursorShapeDeviceV1>)>,
    surfaces: Vec<Surface>,
    start: Instant,
}

impl<S: Shell> Host<S> {
    /// Connects to the compositor and returns the host and an event loop
    /// with the Wayland connection in it. Add other sources (timers,
    /// channels) to the loop, then call [`Host::run`].
    pub fn connect(options: Options, shell: S) -> Result<(Self, EventLoop<'static, Self>), String> {
        let conn =
            Connection::connect_to_env().map_err(|e| format!("no Wayland compositor: {e}"))?;
        let (globals, queue) = registry_queue_init(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let event_loop = EventLoop::try_new().map_err(|e| e.to_string())?;
        WaylandSource::new(conn, queue)
            .insert(event_loop.handle())
            .map_err(|e| e.to_string())?;

        let compositor =
            CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
        let layer_shell = LayerShell::bind(&globals, &qh)
            .map_err(|e| format!("the compositor has no layer shell (zwlr_layer_shell_v1): {e}"))?;
        let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
        let pool = SlotPool::new(4096 * 32 * 4, &shm).map_err(|e| e.to_string())?;
        let host = Self {
            shell,
            options,
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state: OutputState::new(&globals, &qh),
            compositor,
            layer_shell,
            shm,
            pool,
            cursor_shape: CursorShapeManager::bind(&globals, &qh).ok(),
            pointer: None,
            surfaces: Vec::new(),
            start: Instant::now(),
            qh,
        };
        Ok((host, event_loop))
    }

    /// Runs until an error, such as the compositor going away.
    pub fn run(mut self, mut event_loop: EventLoop<'static, Self>) -> Result<(), String> {
        loop {
            let timeout = self
                .next_repaint()
                .map(|t| t.saturating_duration_since(Instant::now()));
            event_loop
                .dispatch(timeout, &mut self)
                .map_err(|e| e.to_string())?;
            let now = Instant::now();
            for s in &mut self.surfaces {
                if s.repaint_at.is_some_and(|t| t <= now) {
                    s.repaint_at = None;
                    s.dirty = true;
                }
            }
            self.draw_dirty();
        }
    }

    /// Redraws every surface, e.g. after the shell's state changed.
    pub fn redraw_all(&mut self) {
        for s in &mut self.surfaces {
            s.dirty = true;
        }
    }

    fn next_repaint(&self) -> Option<Instant> {
        self.surfaces.iter().filter_map(|s| s.repaint_at).min()
    }

    fn draw_dirty(&mut self) {
        for i in 0..self.surfaces.len() {
            let s = &self.surfaces[i];
            if s.dirty && s.configured && !s.frame_pending {
                self.draw(i);
            }
        }
    }

    fn draw(&mut self, i: usize) {
        let Self {
            shell,
            surfaces,
            pool,
            start,
            qh,
            ..
        } = self;
        let s = &mut surfaces[i];
        s.dirty = false;
        let scale = s.scale.max(1);
        let (pw, ph) = (s.width as i32 * scale, s.height as i32 * scale);
        if pw <= 0 || ph <= 0 {
            return;
        }

        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(s.width as f32, s.height as f32),
            )),
            time: Some(start.elapsed().as_secs_f64()),
            events: std::mem::take(&mut s.events),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale as f32);
        let out = s.ctx.run_ui(input, |ui| shell.ui(ui, &s.name));

        if let Some(v) = out.viewport_output.get(&egui::ViewportId::ROOT) {
            if v.repaint_delay.is_zero() {
                s.dirty = true;
            } else if v.repaint_delay < Duration::from_secs(3600) {
                s.repaint_at = Some(Instant::now() + v.repaint_delay);
            }
        }

        let primitives = s.ctx.tessellate(out.shapes, out.pixels_per_point);
        s.pixels.resize((pw * ph) as usize, 0);
        s.painter.paint(
            &mut s.pixels,
            pw as usize,
            ph as usize,
            out.pixels_per_point,
            &primitives,
            &out.textures_delta,
        );

        let (buffer, canvas) = match pool.create_buffer(pw, ph, pw * 4, wl_shm::Format::Argb8888) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("patina-bar: no {pw}x{ph} buffer: {e}");
                return;
            }
        };
        for (dst, src) in canvas.chunks_exact_mut(4).zip(&s.pixels) {
            dst.copy_from_slice(&src.to_le_bytes());
        }
        let surface = s.layer.wl_surface();
        surface.set_buffer_scale(scale);
        surface.damage_buffer(0, 0, pw, ph);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        if let Err(e) = buffer.attach_to(surface) {
            eprintln!("patina-bar: {e}");
            return;
        }
        s.layer.commit();
        s.frame_pending = true;
    }

    fn add_surface(&mut self, output: wl_output::WlOutput) {
        let info = self.output_state.info(&output);
        let name = info
            .as_ref()
            .and_then(|i| i.name.clone())
            .unwrap_or_default();
        let scale = info.as_ref().map_or(1, |i| i.scale_factor);
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            self.options.layer,
            Some(self.options.namespace.clone()),
            Some(&output),
        );
        layer.set_anchor(self.options.edge | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, self.options.height);
        layer.set_exclusive_zone(self.options.height as i32);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();

        let ctx = egui::Context::default();
        self.shell.setup(&ctx);
        self.surfaces.push(Surface {
            output,
            name,
            layer,
            ctx,
            painter: Painter::default(),
            pixels: Vec::new(),
            width: 0,
            height: self.options.height,
            scale,
            configured: false,
            dirty: true,
            frame_pending: false,
            repaint_at: None,
            events: Vec::new(),
        });
    }

    fn surface_mut(&mut self, surface: &wl_surface::WlSurface) -> Option<&mut Surface> {
        self.surfaces
            .iter_mut()
            .find(|s| s.layer.wl_surface() == surface)
    }
}

impl<S: Shell> CompositorHandler for Host<S> {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.scale = factor;
            s.dirty = true;
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if let Some(s) = self.surface_mut(surface) {
            s.frame_pending = false;
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

impl<S: Shell> OutputHandler for Host<S> {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.add_surface(output);
    }

    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        let Some(info) = self.output_state.info(&output) else {
            return;
        };
        if let Some(s) = self.surfaces.iter_mut().find(|s| s.output == output) {
            s.name = info.name.unwrap_or_default();
            s.dirty = true;
        }
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces.retain(|s| s.output != output);
    }
}

impl<S: Shell> LayerShellHandler for Host<S> {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.surfaces.retain(|s| &s.layer != layer);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(s) = self.surfaces.iter_mut().find(|s| &s.layer == layer) else {
            return;
        };
        let (w, h) = configure.new_size;
        if w > 0 {
            s.width = w;
        }
        if h > 0 {
            s.height = h;
        }
        s.configured = true;
        s.dirty = true;
    }
}

impl<S: Shell> SeatHandler for Host<S> {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && self.pointer.is_none()
            && let Ok(pointer) = self.seat_state.get_pointer(qh, &seat)
        {
            let device = self
                .cursor_shape
                .as_ref()
                .map(|m| m.get_shape_device(&pointer, qh));
            self.pointer = Some((pointer, device));
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Some((pointer, device)) = self.pointer.take()
        {
            if let Some(device) = device {
                device.destroy();
            }
            pointer.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl<S: Shell> PointerHandler for Host<S> {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let pos = egui::pos2(event.position.0 as f32, event.position.1 as f32);
            let egui_event = match event.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some((_, Some(device))) = &self.pointer {
                        device.set_shape(serial, Shape::Default);
                    }
                    egui::Event::PointerMoved(pos)
                }
                PointerEventKind::Leave { .. } => egui::Event::PointerGone,
                PointerEventKind::Motion { .. } => egui::Event::PointerMoved(pos),
                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let Some(button) = egui_button(button) else {
                        continue;
                    };
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: matches!(event.kind, PointerEventKind::Press { .. }),
                        modifiers: egui::Modifiers::default(),
                    }
                }
                PointerEventKind::Axis { .. } => continue,
            };
            if let Some(s) = self.surface_mut(&event.surface) {
                s.events.push(egui_event);
                s.dirty = true;
            }
        }
    }
}

/// Linux evdev button codes, which Wayland uses on every system.
fn egui_button(code: u32) -> Option<egui::PointerButton> {
    match code {
        0x110 => Some(egui::PointerButton::Primary),
        0x111 => Some(egui::PointerButton::Secondary),
        0x112 => Some(egui::PointerButton::Middle),
        0x113 => Some(egui::PointerButton::Extra1),
        0x114 => Some(egui::PointerButton::Extra2),
        _ => None,
    }
}

impl<S: Shell> ShmHandler for Host<S> {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl<S: Shell> ProvidesRegistryState for Host<S> {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(@<S: Shell> Host<S>);
smithay_client_toolkit::delegate_dispatch2!(@<S: Shell> Host<S>);
