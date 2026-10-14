//! Running on the hardware, as a login session or from a text console:
//! the GPU through DRM/KMS and GBM, rendered with GLES, input through
//! libinput, and devices from the login session through libseat (ADR-0016).
//!
//! One GPU and one monitor for now: the internal panel if there is one,
//! otherwise the first connected monitor.

use std::time::Duration;

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            compositor::{DrmCompositor, FrameFlags},
            exporter::gbm::GbmFramebufferExporter,
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma, ImportMemWl,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                render_elements,
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{all_gpus, primary_gpu},
    },
    desktop::{
        layer_map_for_output,
        space::{SpaceRenderElements, space_render_elements},
        utils::send_frames_surface_tree,
    },
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::{
            EventLoop, LoopHandle,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as _, ModeTypeFlags, connector, crtc},
        input::{DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_server::DisplayHandle,
    },
    utils::{DeviceFd, Physical, Point, Transform},
    wayland::{
        compositor::with_states,
        dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufState},
    },
};

use crate::input::KeyAction;
use crate::nested::BACKGROUND;
use crate::state::State;

type Compositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

render_elements! {
    pub OutputElement<=GlesRenderer>;
    Space=SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Cursor=MemoryRenderBufferRenderElement<GlesRenderer>,
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
}

/// The formats a monitor's frames may use, best first.
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Xrgb8888];

/// When nothing changed on screen there's no vblank to wait for; the next
/// frame is then held back this long, so clients that redraw on every frame
/// callback don't spin.
const IDLE_FRAME: Duration = Duration::from_millis(16);

pub struct UdevData {
    session: LibSeatSession,
    libinput: Libinput,
    drm: DrmDevice,
    pub(crate) renderer: GlesRenderer,
    compositor: Compositor,
    pub(crate) dmabuf_state: DmabufState,
    _dmabuf_global: DmabufGlobal,
    cursor: Cursor,
    /// Touchpads, to apply `[touchpad]` to again on a reload.
    touchpads: Vec<smithay::reexports::input::Device>,
    loop_handle: LoopHandle<'static, State>,
    /// The session has the devices (we're the active console).
    active: bool,
    /// A frame is queued, or held back, and the next must wait.
    frame_pending: bool,
}

/// A monitor to drive: its connector, a CRTC for it, and its mode.
struct Monitor {
    connector: connector::Info,
    crtc: crtc::Handle,
    mode: smithay::reexports::drm::control::Mode,
}

/// Opens the session, the primary GPU and its first monitor, and libinput,
/// adds their events to the loop, and returns the backend with the output
/// to give the compositor.
pub fn init(
    event_loop: &EventLoop<'static, State>,
    dh: &DisplayHandle,
    config: &crate::config::Config,
) -> Result<(UdevData, Output, f64), String> {
    let (mut session, notifier) =
        LibSeatSession::new().map_err(|e| format!("no login session to run in: {e}"))?;
    let seat = session.seat();

    let path = primary_gpu(&seat)
        .ok()
        .flatten()
        .or_else(|| all_gpus(&seat).ok()?.into_iter().next())
        .ok_or("no GPU found")?;
    let fd = session
        .open(
            &path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, drm_notifier) =
        DrmDevice::new(fd.clone(), true).map_err(|e| format!("{}: {e}", path.display()))?;
    let gbm = GbmDevice::new(fd.clone()).map_err(|e| format!("gbm: {e}"))?;
    // SAFETY: the GBM device lives as long as the display and renderer.
    let display = unsafe { EGLDisplay::new(gbm.clone()) }.map_err(|e| format!("egl: {e}"))?;
    let context = EGLContext::new(&display).map_err(|e| format!("egl context: {e}"))?;
    // SAFETY: the context is current only on this thread.
    let renderer = unsafe { GlesRenderer::new(context) }.map_err(|e| format!("gles: {e}"))?;

    let monitor = choose_monitor(&drm).ok_or("no monitor is connected")?;
    let (output, auto_scale) = make_output(&monitor, config);
    let surface = drm
        .create_surface(monitor.crtc, monitor.mode, &[monitor.connector.handle()])
        .map_err(|e| format!("can't drive {}: {e}", output.name()))?;
    let render_node = DrmNode::from_path(&path)
        .ok()
        .and_then(|n| n.node_with_type(NodeType::Render).and_then(Result::ok));
    let compositor = Compositor::new(
        &output,
        surface,
        None,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        GbmFramebufferExporter::new(gbm.clone(), render_node),
        COLOR_FORMATS,
        renderer.egl_context().dmabuf_render_formats().clone(),
        drm.cursor_size(),
        Some(gbm.clone()),
    )
    .map_err(|e| format!("can't drive {}: {e:?}", output.name()))?;

    let formats = renderer.dmabuf_formats();
    let mut dmabuf_state = DmabufState::new();
    let feedback_node =
        render_node.unwrap_or_else(|| DrmNode::from_path(&path).expect("opened above"));
    let dmabuf_global = match DmabufFeedbackBuilder::new(feedback_node.dev_id(), formats.clone())
        .build()
    {
        Ok(feedback) => dmabuf_state.create_global_with_default_feedback::<State>(dh, &feedback),
        Err(_) => dmabuf_state.create_global::<State>(dh, formats),
    };

    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|()| format!("libinput can't take seat {seat}"))?;

    let handle = event_loop.handle();
    handle
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            |event, _, state| libinput_event(state, event),
        )
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(notifier, |event, _, state| session_event(state, event))
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(drm_notifier, |event, _, state| match event {
            DrmEvent::VBlank(_) => vblank(state),
            DrmEvent::Error(e) => eprintln!("patina-compositor: drm: {e}"),
        })
        .map_err(|e| e.to_string())?;

    eprintln!(
        "patina-compositor: on {} at {}x{}, scale {}",
        output.name(),
        monitor.mode.size().0,
        monitor.mode.size().1,
        output.current_scale().fractional_scale()
    );
    let cursor = Cursor::load(output.current_scale().fractional_scale());
    Ok((
        UdevData {
            session,
            libinput,
            drm,
            renderer,
            compositor,
            dmabuf_state,
            _dmabuf_global: dmabuf_global,
            cursor,
            touchpads: Vec::new(),
            loop_handle: handle,
            active: true,
            frame_pending: false,
        },
        output,
        auto_scale,
    ))
}

impl UdevData {
    /// Applies `[touchpad]` to every touchpad.
    pub fn configure_touchpads(&mut self, settings: &crate::config::Touchpad) {
        for device in &mut self.touchpads {
            configure_touchpad(device, settings);
        }
    }

    /// Loads the pointer again, at the size for `scale`.
    pub fn reload_cursor(&mut self, scale: f64) {
        self.cursor = Cursor::load(scale);
    }
}

fn configure_touchpad(
    device: &mut smithay::reexports::input::Device,
    settings: &crate::config::Touchpad,
) {
    let _ = device.config_tap_set_enabled(settings.tap);
    let _ = device.config_scroll_set_natural_scroll_enabled(settings.natural_scroll);
    let _ = device.config_accel_set_speed(settings.speed);
}

/// The internal panel if it's connected (a laptop), otherwise the first
/// connected monitor; its preferred mode, and a CRTC that can drive it.
fn choose_monitor(drm: &DrmDevice) -> Option<Monitor> {
    let resources = drm.resource_handles().ok()?;
    let mut connected: Vec<connector::Info> = resources
        .connectors()
        .iter()
        .filter_map(|c| drm.get_connector(*c, false).ok())
        .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
        .collect();
    connected.sort_by_key(|c| !is_internal(c.interface()));
    for connector in connected {
        let crtc = connector
            .encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .next();
        let Some(crtc) = crtc else { continue };
        let mode = connector
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .copied()
            .unwrap_or(connector.modes()[0]);
        return Some(Monitor {
            connector,
            crtc,
            mode,
        });
    }
    None
}

fn is_internal(interface: connector::Interface) -> bool {
    matches!(
        interface,
        connector::Interface::EmbeddedDisplayPort
            | connector::Interface::LVDS
            | connector::Interface::DSI
    )
}

/// The output for `monitor`, named as the kernel names connectors (eDP-1,
/// DP-3), at its mode and scale, and the scale its density alone would
/// give.
fn make_output(monitor: &Monitor, config: &crate::config::Config) -> (Output, f64) {
    let c = &monitor.connector;
    let name = format!("{}-{}", c.interface().as_str(), c.interface_id());
    let (mm_w, mm_h) = c.size().unwrap_or((0, 0));
    let output = Output::new(
        name,
        PhysicalProperties {
            size: (mm_w as i32, mm_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "Unknown".into(),
            model: "Unknown".into(),
        },
    );
    let (w, h) = monitor.mode.size();
    let mode = Mode::from(monitor.mode);
    let automatic = default_scale(w as u32, h as u32, mm_w);
    let scale = crate::config::scale(config, automatic);
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Fractional(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    (output, automatic)
}

/// A scale for a panel `width` x `height` pixels and `width_mm` wide: 2
/// for dense panels that stay at least 800 points tall, 1.5 for laptop
/// panels between, 1 otherwise (and when the size is unknown).
pub fn default_scale(width: u32, height: u32, width_mm: u32) -> f64 {
    if width_mm == 0 {
        return 1.0;
    }
    let dpi = width as f64 / (width_mm as f64 / 25.4);
    if dpi >= 192.0 && height as f64 / 2.0 >= 800.0 {
        2.0
    } else if dpi >= 140.0 {
        1.5
    } else {
        1.0
    }
}

fn libinput_event(state: &mut State, event: InputEvent<LibinputInputBackend>) {
    if let InputEvent::DeviceAdded { mut device } = event {
        // A touchpad is a pointer that can tap; [touchpad] applies to those,
        // not to mice.
        if device.has_capability(DeviceCapability::Pointer) && device.config_tap_finger_count() > 0
        {
            configure_touchpad(&mut device, &state.config.touchpad);
            if let Some(u) = state.udev.as_mut() {
                u.touchpads.push(device);
            }
        }
        return;
    }
    if let InputEvent::DeviceRemoved { device } = &event {
        if let Some(u) = state.udev.as_mut() {
            u.touchpads.retain(|d| d != device);
        }
        return;
    }
    match crate::input::handle(state, event) {
        Some(KeyAction::SwitchVt(vt)) => {
            if let Some(u) = state.udev.as_mut()
                && let Err(e) = u.session.change_vt(vt)
            {
                eprintln!("patina-compositor: can't switch to console {vt}: {e}");
            }
        }
        Some(KeyAction::Quit) => state.loop_signal.stop(),
        // Done inside State::key.
        Some(
            KeyAction::NextLayout
            | KeyAction::Reload
            | KeyAction::Media(_)
            | KeyAction::Window(_)
            | KeyAction::Run(_)
            | KeyAction::Snap(_)
            | KeyAction::CloseWindow,
        )
        | None => {}
    }
}

/// Switching consoles: give the devices up while another console has them,
/// and take them back, redrawing everything, when we're shown again.
fn session_event(state: &mut State, event: SessionEvent) {
    let Some(u) = state.udev.as_mut() else { return };
    match event {
        SessionEvent::PauseSession => {
            u.active = false;
            u.libinput.suspend();
            u.drm.pause();
        }
        SessionEvent::ActivateSession => {
            if u.libinput.resume().is_err() {
                eprintln!("patina-compositor: libinput didn't resume");
            }
            if let Err(e) = u.drm.activate(false) {
                eprintln!("patina-compositor: drm didn't resume: {e}");
            }
            if let Err(e) = u.compositor.reset_state() {
                eprintln!("patina-compositor: {e}");
            }
            u.active = true;
            u.frame_pending = false;
            state.needs_redraw = true;
        }
    }
}

fn vblank(state: &mut State) {
    let Some(u) = state.udev.as_mut() else { return };
    if let Err(e) = u.compositor.frame_submitted() {
        eprintln!("patina-compositor: {e:?}");
    }
    u.frame_pending = false;
}

/// Draws a frame if something changed and the monitor is ready for one.
pub fn schedule(state: &mut State) {
    let ready = state
        .udev
        .as_ref()
        .is_some_and(|u| u.active && !u.frame_pending);
    if ready && state.needs_redraw {
        render(state);
    }
}

fn render(state: &mut State) {
    let Some(u) = state.udev.as_mut() else { return };
    state.needs_redraw = false;
    let scale = state.output.current_scale().fractional_scale();

    let mut elements: Vec<OutputElement> = Vec::new();
    let pointer = state
        .seat
        .get_pointer()
        .map(|p| p.current_location())
        .unwrap_or_default();
    elements.extend(
        u.cursor
            .elements(&mut u.renderer, &state.cursor, pointer, scale),
    );
    match space_render_elements(&mut u.renderer, [&state.space], &state.output, 1.0) {
        Ok(space) => elements.extend(space.into_iter().map(OutputElement::Space)),
        Err(e) => eprintln!("patina-compositor: {e:?}"),
    }

    let queued =
        match u
            .compositor
            .render_frame(&mut u.renderer, &elements, BACKGROUND, FrameFlags::DEFAULT)
        {
            Ok(frame) if !frame.is_empty => match u.compositor.queue_frame(()) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("patina-compositor: {e:?}");
                    false
                }
            },
            Ok(_) => false,
            Err(e) => {
                eprintln!("patina-compositor: render failed: {e:?}");
                false
            }
        };
    u.frame_pending = true;
    if !queued {
        // No vblank is coming; hold the next frame back a little instead.
        let _ = u
            .loop_handle
            .insert_source(Timer::from_duration(IDLE_FRAME), |_, _, state| {
                if let Some(u) = state.udev.as_mut() {
                    u.frame_pending = false;
                }
                TimeoutAction::Drop
            });
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
    if let CursorImageStatus::Surface(surface) = &state.cursor {
        send_frames_surface_tree(surface, &output, time, Some(Duration::ZERO), |_, _| {
            Some(output.clone())
        });
    }
    state.space.refresh();
    state.popups.cleanup();
}

/// The pointer: the client's own cursor surface when it sets one, otherwise
/// the cursor theme's arrow (`XCURSOR_THEME`, `XCURSOR_SIZE`), otherwise a
/// plain drawn one.
struct Cursor {
    image: MemoryRenderBuffer,
    hotspot: Point<i32, Physical>,
}

impl Cursor {
    fn load(scale: f64) -> Self {
        let size = std::env::var("XCURSOR_SIZE")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(24);
        let size = (size as f64 * scale).round() as u32;
        let theme = std::env::var("XCURSOR_THEME").unwrap_or_else(|_| "default".into());
        let themed = xcursor::CursorTheme::load(&theme)
            .load_icon("default")
            .or_else(|| xcursor::CursorTheme::load(&theme).load_icon("left_ptr"))
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| xcursor::parser::parse_xcursor(&bytes))
            .and_then(|images| {
                images
                    .into_iter()
                    .min_by_key(|i| (i.size as i64 - size as i64).abs())
            });
        match themed {
            Some(i) => Self {
                image: MemoryRenderBuffer::from_slice(
                    &i.pixels_rgba,
                    Fourcc::Argb8888,
                    (i.width as i32, i.height as i32),
                    1,
                    Transform::Normal,
                    None,
                ),
                hotspot: (i.xhot as i32, i.yhot as i32).into(),
            },
            None => Self::drawn(size.max(8)),
        }
    }

    /// A white arrow with a black edge, for systems without a cursor theme.
    fn drawn(size: u32) -> Self {
        let s = size as i32;
        let mut pixels = vec![0u8; (s * s * 4) as usize];
        for y in 0..s {
            for x in 0..s {
                // The arrow is the triangle x <= y, cut at three quarters.
                let inside = x <= y && y < s * 3 / 4 + x / 3;
                if !inside {
                    continue;
                }
                let edge = x == 0 || x == y || y == s * 3 / 4 + x / 3 - 1;
                let v = if edge { 0 } else { 255 };
                let i = ((y * s + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        Self {
            image: MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Argb8888,
                (s, s),
                1,
                Transform::Normal,
                None,
            ),
            hotspot: (0, 0).into(),
        }
    }

    fn elements(
        &self,
        renderer: &mut GlesRenderer,
        status: &CursorImageStatus,
        pointer: Point<f64, smithay::utils::Logical>,
        scale: f64,
    ) -> Vec<OutputElement> {
        let at = pointer.to_physical(scale);
        match status {
            CursorImageStatus::Hidden => Vec::new(),
            CursorImageStatus::Surface(surface) => {
                let hotspot = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<std::sync::Mutex<CursorImageAttributes>>()
                        .and_then(|a| a.lock().ok().map(|a| a.hotspot))
                        .unwrap_or_default()
                });
                let at = (at - hotspot.to_f64().to_physical(scale)).to_i32_round();
                render_elements_from_surface_tree(renderer, surface, at, scale, 1.0, Kind::Cursor)
            }
            CursorImageStatus::Named(_) => {
                let at = at - self.hotspot.to_f64();
                MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    at,
                    &self.image,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                )
                .map(|e| vec![OutputElement::Cursor(e)])
                .unwrap_or_default()
            }
        }
    }
}

/// The shm formats the GL renderer reads, for clients drawing in software.
pub fn shm_formats(
    data: &mut UdevData,
) -> Vec<smithay::reexports::wayland_server::protocol::wl_shm::Format> {
    data.renderer.shm_formats().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_follows_density() {
        // A 24" 1920x1080 monitor, about 92 dpi.
        assert_eq!(default_scale(1920, 1080, 531), 1.0);
        // A 14" 1920x1200 laptop panel, about 162 dpi.
        assert_eq!(default_scale(1920, 1200, 301), 1.5);
        // A 13" 2880x1800 panel, about 254 dpi and 900 points tall at 2x.
        assert_eq!(default_scale(2880, 1800, 288), 2.0);
        // A dense but short panel stays at 1.5 rather than 2.
        assert_eq!(default_scale(2560, 1440, 290), 1.5);
        // Unknown size: 1.
        assert_eq!(default_scale(3840, 2160, 0), 1.0);
    }
}
