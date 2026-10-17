//! Runs the compositor as a window on another desktop, rendered with OpenGL
//! ES through winit and EGL, with linux-dmabuf so clients hand over GPU
//! buffers instead of drawing in software (ADR-0015). The pixman backend in
//! `nested.rs` is the fallback where there's no EGL.

use std::time::Duration;

use smithay::{
    backend::{
        egl::EGLDevice,
        renderer::{ImportDma, ImportMemWl, damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self as smithay_winit, WinitEvent, WinitEventLoop, WinitGraphicsBackend},
    },
    desktop::{layer_map_for_output, space::render_output},
    output::{Mode, Output, Scale},
    reexports::{
        calloop::{
            EventIterator, EventLoop, EventSource, LoopSignal, Poll, PostAction, Readiness, Token,
            TokenFactory,
        },
        wayland_server::DisplayHandle,
        winit::{dpi::LogicalSize, platform::wayland::WindowAttributesExtWayland, window::Window},
    },
    utils::{Physical, Size, Transform},
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufState},
};

use crate::nested::{BACKGROUND, DEFAULT_SIZE};
use crate::state::State;

const APP_ID: &str = "com.sectorflabs.PatinaCompositor";

pub struct WinitData {
    pub(crate) backend: WinitGraphicsBackend<GlesRenderer>,
    damage_tracker: OutputDamageTracker,
    pub(crate) dmabuf_state: DmabufState,
    _dmabuf_global: DmabufGlobal,
    /// Frames to draw in full, after a resize, before trusting buffer age.
    full_redraw: u8,
    /// A redraw was asked of winit and hasn't come yet.
    redraw_requested: bool,
}

/// The parent window's scale, unless the config or `PATINA_SCALE` say
/// otherwise (as on the hardware), so fractional scales can be tried on any
/// desktop.
fn scale(config: &crate::config::Config, window_scale: f64) -> Scale {
    Scale::Fractional(crate::config::scale(config, window_scale))
}

/// The parent window's own scale, which the compositor would use by itself.
pub fn window_scale(data: &WinitData) -> f64 {
    data.backend.scale_factor()
}

fn mode(size: Size<i32, Physical>) -> Mode {
    Mode {
        size,
        refresh: 60_000,
    }
}

/// Opens the window and its GL context, sets `output` to its size and
/// scale, offers linux-dmabuf, and adds winit's events to the loop. Fails
/// where there's no EGL, and the caller falls back to pixman.
pub fn init(
    event_loop: &EventLoop<'static, State>,
    output: &Output,
    dh: &DisplayHandle,
    config: &crate::config::Config,
) -> Result<WinitData, String> {
    let attributes = Window::default_attributes()
        .with_inner_size(LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1))
        .with_title("Patina")
        .with_name(APP_ID, "")
        .with_visible(true);
    let (mut backend, winit) = smithay_winit::init_from_attributes::<GlesRenderer>(attributes)
        .map_err(|e| e.to_string())?;

    let mode = mode(backend.window_size());
    // GL draws upside down; winit's output is flipped to match, as in
    // Smithay's own examples.
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        Some(scale(config, backend.scale_factor())),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    // Feedback names the GPU, so clients allocate buffers it can read;
    // without a render node, plain dmabuf v3.
    let formats = backend.renderer().dmabuf_formats();
    let node = EGLDevice::device_for_display(backend.renderer().egl_context().display())
        .ok()
        .and_then(|device| device.try_get_render_node().ok().flatten());
    let mut dmabuf_state = DmabufState::new();
    let feedback = node.and_then(|n| {
        DmabufFeedbackBuilder::new(n.dev_id(), formats.clone())
            .build()
            .ok()
    });
    let dmabuf_global = match feedback {
        Some(feedback) => dmabuf_state.create_global_with_default_feedback::<State>(dh, &feedback),
        None => dmabuf_state.create_global::<State>(dh, formats),
    };

    let winit = UntilExit {
        winit,
        signal: event_loop.get_signal(),
    };
    event_loop
        .handle()
        .insert_source(winit, |event, _, state| handle_event(state, event))
        .map_err(|e| e.to_string())?;

    Ok(WinitData {
        damage_tracker: OutputDamageTracker::from_output(output),
        backend,
        dmabuf_state,
        _dmabuf_global: dmabuf_global,
        full_redraw: 0,
        redraw_requested: false,
    })
}

/// The shm formats the GL renderer reads, for clients drawing in software.
pub fn shm_formats(
    data: &mut WinitData,
) -> Vec<smithay::reexports::wayland_server::protocol::wl_shm::Format> {
    data.backend.renderer().shm_formats().collect()
}

/// Asks winit for a frame if something changed. The frame comes as
/// `Redraw`, paced by the parent desktop.
pub fn schedule(state: &mut State) {
    if !state.needs_redraw {
        return;
    }
    if let Some(w) = state.winit.as_mut()
        && !w.redraw_requested
    {
        w.redraw_requested = true;
        w.backend.window().request_redraw();
    }
}

/// winit's events, quitting the compositor when winit exits: the desktop
/// it's nested in has gone (bsdt's restart stops dwl; the parent's session
/// ends). Smithay's source only removes itself then, which left the
/// compositor running with no window. It's a clean exit, so patina-session
/// ends too, rather than starting it again in a desktop that isn't there.
struct UntilExit {
    winit: WinitEventLoop,
    signal: LoopSignal,
}

impl EventSource for UntilExit {
    type Event = WinitEvent;
    type Metadata = ();
    type Ret = ();
    type Error = std::io::Error;

    const NEEDS_EXTRA_LIFECYCLE_EVENTS: bool = true;

    fn before_sleep(&mut self) -> smithay::reexports::calloop::Result<Option<(Readiness, Token)>> {
        self.winit.before_sleep()
    }

    fn before_handle_events(&mut self, events: EventIterator<'_>) {
        self.winit.before_handle_events(events)
    }

    fn process_events<F>(
        &mut self,
        readiness: Readiness,
        token: Token,
        callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        let action = self.winit.process_events(readiness, token, callback)?;
        if matches!(action, PostAction::Remove) {
            eprintln!("patina-compositor: the desktop it ran in has gone; quitting");
            self.signal.stop();
        }
        Ok(action)
    }

    fn register(
        &mut self,
        poll: &mut Poll,
        factory: &mut TokenFactory,
    ) -> smithay::reexports::calloop::Result<()> {
        self.winit.register(poll, factory)
    }

    fn reregister(
        &mut self,
        poll: &mut Poll,
        factory: &mut TokenFactory,
    ) -> smithay::reexports::calloop::Result<()> {
        self.winit.reregister(poll, factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> smithay::reexports::calloop::Result<()> {
        self.winit.unregister(poll)
    }
}

fn handle_event(state: &mut State, event: WinitEvent) {
    match event {
        WinitEvent::Resized { size, scale_factor } => {
            state.auto_scale = scale_factor;
            let mode = mode(size);
            state.output.change_current_state(
                Some(mode),
                None,
                Some(scale(&state.config, scale_factor)),
                None,
            );
            state.output.set_preferred(mode);
            layer_map_for_output(&state.output).arrange();
            state.relayout_maximized();
            state.update_preferred_scale();
            if let Some(w) = state.winit.as_mut() {
                w.full_redraw = 4;
            }
            state.needs_redraw = true;
        }
        WinitEvent::Input(event) => {
            if crate::input::handle(state, event) == Some(crate::input::KeyAction::Quit) {
                state.loop_signal.stop();
            }
        }
        WinitEvent::Redraw => {
            if let Some(w) = state.winit.as_mut() {
                w.redraw_requested = false;
            }
            render(state);
        }
        WinitEvent::CloseRequested => state.loop_signal.stop(),
        WinitEvent::Focus(_) => {}
    }
}

/// Draws the output with GL, presents what changed, and tells clients to
/// draw their next frame.
fn render(state: &mut State) {
    let locked = state.locked();
    let lock_surface = state.lock_surface().cloned();
    let scale = state.output.current_scale().fractional_scale();
    if state.winit.is_none() {
        return;
    }
    state.needs_redraw = false;
    // A screenshot's flash, then where a drop would tile (ADR-0049).
    let flash: Vec<_> = state
        .flash_element()
        .into_iter()
        .chain(state.preview_element())
        .collect();
    let dnd_icon = state.dnd_icon_at();
    let fullscreen = state.fullscreen_drawn();
    // Locked, only the locker, over black.
    let spaces: Vec<&smithay::desktop::Space<smithay::desktop::Window>> = if locked {
        Vec::new()
    } else {
        vec![&state.space]
    };
    let Some(w) = state.winit.as_mut() else {
        return;
    };
    let age = if w.full_redraw > 0 {
        w.full_redraw -= 1;
        0
    } else {
        w.backend.buffer_age().unwrap_or(0)
    };
    let damage = match w.backend.bind() {
        Ok((renderer, mut framebuffer)) => {
            let overlay = crate::screenshot::overlay(
                renderer,
                lock_surface.as_ref(),
                scale,
                flash,
                dnd_icon.as_ref(),
                fullscreen.as_ref(),
            );
            match render_output::<_, crate::screenshot::Overlay<GlesRenderer>, _, _>(
                &state.output,
                renderer,
                &mut framebuffer,
                1.0,
                age,
                spaces,
                &overlay,
                &mut w.damage_tracker,
                BACKGROUND,
            ) {
                Ok(result) => result.damage.cloned(),
                Err(e) => {
                    eprintln!("patina-compositor: render failed: {e:?}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("patina-compositor: {e}");
            None
        }
    };
    if let Some(damage) = damage
        && let Err(e) = w.backend.submit(Some(&damage))
    {
        eprintln!("patina-compositor: {e}");
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
