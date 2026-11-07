//! Running on the hardware, as a login session or from a text console:
//! the GPU through DRM/KMS and GBM, rendered with GLES, input through
//! libinput, and devices from the login session through libseat (ADR-0016).
//!
//! One GPU, and every monitor connected to it at start-up (ADR-0121): the
//! internal panel first, if there is one, is the primary. Each monitor is a
//! [`Head`] with its own DRM compositor; the desktop is drawn on each,
//! offset by where its output sits in the space.

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
                memory::MemoryRenderBufferRenderElement, render_elements,
                surface::WaylandSurfaceRenderElement,
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{all_gpus, primary_gpu},
    },
    desktop::{layer_map_for_output, utils::send_frames_surface_tree},
    input::pointer::CursorImageStatus,
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
    utils::{DeviceFd, Transform},
    wayland::{
        dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufState},
        drm_syncobj::{DrmSyncobjState, supports_syncobj_eventfd},
    },
};

use crate::input::KeyAction;
use crate::nested::BACKGROUND;
use crate::state::State;

type Compositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

render_elements! {
    pub OutputElement<=GlesRenderer>;
    Desktop=crate::frames::DesktopElement<GlesRenderer>,
    Cursor=MemoryRenderBufferRenderElement<GlesRenderer>,
    Flash=smithay::backend::renderer::element::solid::SolidColorRenderElement,
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
    /// The monitors, the primary first.
    heads: Vec<Head>,
    pub(crate) dmabuf_state: DmabufState,
    /// linux-drm-syncobj, where the device supports it (`sync.rs`).
    pub(crate) syncobj_state: Option<DrmSyncobjState>,
    _dmabuf_global: DmabufGlobal,
    pub cursors: crate::cursor::Cursors,
    /// Touchpads, to apply `[touchpad]` to again on a reload.
    touchpads: Vec<smithay::reexports::input::Device>,
    loop_handle: LoopHandle<'static, State>,
    /// The session has the devices (we're the active console).
    active: bool,
    /// The monitors are powered off for want of input (ADR-0047); nothing
    /// is drawn until they're on again.
    off: bool,
}

/// One monitor being driven.
struct Head {
    output: Output,
    crtc: crtc::Handle,
    compositor: Compositor,
    /// A frame is queued, or held back, and the next must wait.
    frame_pending: bool,
    /// Something changed since this monitor was last drawn.
    dirty: bool,
    /// The monitor's gamma ramp, for Night Light (ADR-0039); `None` when
    /// its CRTC has none.
    gamma: Option<Gamma>,
}

/// A CRTC's gamma ramp: as it was when we came, and the temperature it's
/// warmed to now.
struct Gamma {
    crtc: crtc::Handle,
    original: [Vec<u16>; 3],
    /// In whole kelvin, so a fade's steps that round the same aren't set
    /// twice.
    shown: u32,
}

impl Gamma {
    /// The ramp `crtc` has, if it has one.
    fn read(drm: &DrmDevice, crtc: crtc::Handle) -> Option<Self> {
        let size = drm.get_crtc(crtc).ok()?.gamma_length() as usize;
        if size == 0 {
            return None;
        }
        let mut ramps = [vec![0u16; size], vec![0u16; size], vec![0u16; size]];
        let [r, g, b] = &mut ramps;
        if let Err(e) = drm.get_gamma(crtc, r, g, b) {
            eprintln!("patina-compositor: can't read the gamma ramp, so no night light: {e}");
            return None;
        }
        // A ramp left all zeros by whoever was here before would blank
        // the screen; start from a straight one instead.
        if ramps.iter().all(|c| c.iter().all(|&v| v == 0)) {
            let straight: Vec<u16> = (0..size)
                .map(|i| (i * 0xffff / (size - 1).max(1)) as u16)
                .collect();
            ramps = [straight.clone(), straight.clone(), straight];
        }
        Some(Self {
            crtc,
            original: ramps,
            shown: crate::night_light::NEUTRAL,
        })
    }

    fn set(&self, drm: &DrmDevice, kelvin: u32) -> std::io::Result<()> {
        let [r, g, b] = crate::night_light::warm_ramps(&self.original, kelvin as f64);
        drm.set_gamma(self.crtc, &r, &g, &b)
    }
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

    let monitors = choose_monitors(&drm);
    if monitors.is_empty() {
        return Err("no monitor is connected".into());
    }
    let render_node = DrmNode::from_path(&path)
        .ok()
        .and_then(|n| n.node_with_type(NodeType::Render).and_then(Result::ok));
    let mut heads = Vec::new();
    let mut auto_scale = 1.0;
    for (i, monitor) in monitors.iter().enumerate() {
        let gamma = Gamma::read(&drm, monitor.crtc);
        let (output, automatic) = make_output(monitor, config, i == 0);
        let surface = match drm.create_surface(
            monitor.crtc,
            monitor.mode,
            &[monitor.connector.handle()],
        ) {
            Ok(surface) => surface,
            Err(e) if i > 0 => {
                eprintln!("patina-compositor: can't drive {}: {e}", output.name());
                continue;
            }
            Err(e) => return Err(format!("can't drive {}: {e}", output.name())),
        };
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
        );
        let compositor = match compositor {
            Ok(c) => c,
            Err(e) if i > 0 => {
                eprintln!("patina-compositor: can't drive {}: {e:?}", output.name());
                continue;
            }
            Err(e) => return Err(format!("can't drive {}: {e:?}", output.name())),
        };
        if i == 0 {
            auto_scale = automatic;
        }
        eprintln!(
            "patina-compositor: on {} at {}x{}, scale {}",
            output.name(),
            monitor.mode.size().0,
            monitor.mode.size().1,
            output.current_scale().fractional_scale()
        );
        heads.push(Head {
            output,
            crtc: monitor.crtc,
            compositor,
            frame_pending: false,
            dirty: true,
            gamma,
        });
    }
    let output = heads[0].output.clone();

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

    // Explicit sync (`sync.rs`): NVIDIA's clients need it not to flicker.
    let syncobj_state =
        supports_syncobj_eventfd(&fd).then(|| DrmSyncobjState::new::<State>(dh, fd.clone()));

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
            DrmEvent::VBlank(crtc) => vblank(state, crtc),
            DrmEvent::Error(e) => eprintln!("patina-compositor: drm: {e}"),
        })
        .map_err(|e| e.to_string())?;

    let cursors = crate::cursor::Cursors::new(
        &config.cursor.theme,
        config.cursor.size,
        output.current_scale().fractional_scale(),
    );
    Ok((
        UdevData {
            session,
            libinput,
            drm,
            renderer,
            heads,
            dmabuf_state,
            syncobj_state,
            _dmabuf_global: dmabuf_global,
            cursors,
            touchpads: Vec::new(),
            loop_handle: handle,
            active: true,
            off: false,
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

    /// Whether Night Light can warm a monitor.
    pub fn has_gamma(&self) -> bool {
        self.heads.iter().any(|h| h.gamma.is_some())
    }

    /// The outputs of the monitors after the primary.
    pub fn extra_outputs(&self) -> Vec<Output> {
        self.heads.iter().skip(1).map(|h| h.output.clone()).collect()
    }

    /// Warms the monitors to `kelvin` (6500 for neutral), now if the session
    /// has the GPU, otherwise when it comes back.
    pub fn set_temperature(&mut self, kelvin: f64) {
        let kelvin = kelvin.round() as u32;
        let mut changed = false;
        for gamma in self.heads.iter_mut().filter_map(|h| h.gamma.as_mut()) {
            if kelvin != gamma.shown {
                gamma.shown = kelvin;
                changed = true;
            }
        }
        if changed && self.active {
            self.apply_gamma();
        }
    }

    /// Sets each ramp for the temperature wanted.
    fn apply_gamma(&mut self) {
        for gamma in self.heads.iter().filter_map(|h| h.gamma.as_ref()) {
            if let Err(e) = gamma.set(&self.drm, gamma.shown) {
                eprintln!("patina-compositor: can't set the gamma ramp: {e}");
            }
        }
    }

    /// Powers the monitors off (DPMS): their planes are cleared, and nothing
    /// is drawn until [`power_on`](Self::power_on).
    pub fn power_off(&mut self) {
        self.off = true;
        if self.active {
            for head in &mut self.heads {
                if let Err(e) = head.compositor.clear() {
                    eprintln!("patina-compositor: can't turn {} off: {e}", head.output.name());
                }
            }
        }
    }

    /// Powers the monitors on again: the next frame lights them.
    pub fn power_on(&mut self) {
        self.off = false;
        for head in &mut self.heads {
            head.frame_pending = false;
            head.dirty = true;
        }
        // As after another console had it.
        if self.active {
            self.apply_gamma();
        }
    }

    /// Makes the cursors again, for `[cursor]` at `scale`.
    pub fn reload_cursors(&mut self, settings: &crate::config::Cursor, scale: f64) {
        self.cursors = crate::cursor::Cursors::new(&settings.theme, settings.size, scale);
    }
}

impl Drop for UdevData {
    /// Leaves the ramps as we found them.
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        for gamma in self.heads.iter().filter_map(|h| h.gamma.as_ref()) {
            if gamma.shown != crate::night_light::NEUTRAL {
                let _ = gamma.set(&self.drm, crate::night_light::NEUTRAL);
            }
        }
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

/// Every connected monitor, the internal panel first (a laptop's), each with
/// its preferred mode and a CRTC of its own that can drive it.
fn choose_monitors(drm: &DrmDevice) -> Vec<Monitor> {
    let Ok(resources) = drm.resource_handles() else {
        return Vec::new();
    };
    let mut connected: Vec<connector::Info> = resources
        .connectors()
        .iter()
        .filter_map(|c| drm.get_connector(*c, false).ok())
        .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
        .collect();
    connected.sort_by_key(|c| !is_internal(c.interface()));
    let mut taken: Vec<crtc::Handle> = Vec::new();
    let mut monitors = Vec::new();
    for connector in connected {
        let crtc = connector
            .encoders()
            .iter()
            .filter_map(|e| drm.get_encoder(*e).ok())
            .flat_map(|e| resources.filter_crtcs(e.possible_crtcs()))
            .find(|c| !taken.contains(c));
        let Some(crtc) = crtc else { continue };
        taken.push(crtc);
        let mode = pick_mode(connector.modes(), connector.size());
        monitors.push(Monitor {
            connector,
            crtc,
            mode,
        });
    }
    monitors
}

/// The mode to run a monitor in: among those whose shape matches the
/// panel's (its size in millimetres), the one it prefers, else the largest;
/// a monitor that takes modes of another shape (a 21:9 panel that accepts
/// 4K) squashes them, and often prefers one. Without a size, or if none
/// match, the preferred mode, else the first.
fn pick_mode(
    modes: &[smithay::reexports::drm::control::Mode],
    size_mm: Option<(u32, u32)>,
) -> smithay::reexports::drm::control::Mode {
    let preferred = |m: &&smithay::reexports::drm::control::Mode| {
        m.mode_type().contains(ModeTypeFlags::PREFERRED)
    };
    let fits = |m: &&smithay::reexports::drm::control::Mode| {
        let (w, h) = m.size();
        match size_mm {
            Some((mw, mh)) if mw > 0 && mh > 0 && h > 0 => {
                let shape = w as f64 / h as f64;
                let panel = mw as f64 / mh as f64;
                (shape / panel - 1.0).abs() < 0.06
            }
            _ => true,
        }
    };
    let size = |m: &&smithay::reexports::drm::control::Mode| {
        let (w, h) = m.size();
        (w as u32 * h as u32, m.vrefresh())
    };
    modes
        .iter()
        .filter(fits)
        .find(preferred)
        .or_else(|| modes.iter().filter(fits).max_by_key(size))
        .or_else(|| modes.iter().find(preferred))
        .unwrap_or(&modes[0])
        .to_owned()
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
///
/// `[screen] scale` is the primary's; another monitor goes by its density.
fn make_output(monitor: &Monitor, config: &crate::config::Config, primary: bool) -> (Output, f64) {
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
    let scale = if primary {
        crate::config::scale(config, automatic)
    } else {
        automatic
    };
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
            | KeyAction::Launcher
            | KeyAction::Hud
            | KeyAction::Spotlight
            | KeyAction::Emoji
            | KeyAction::Reload
            | KeyAction::Restart
            | KeyAction::Media(_)
            | KeyAction::Window(_)
            | KeyAction::Run(_)
            | KeyAction::Snap(_)
            | KeyAction::Grow(_)
            | KeyAction::CycleTabs { .. }
            | KeyAction::ProcessManager
            | KeyAction::Resize(_)
            | KeyAction::SentAsCtrl
            | KeyAction::Remap(_)
            | KeyAction::QuitApp
            | KeyAction::Minimize
            | KeyAction::Lock
            | KeyAction::Screenshot(_)
            | KeyAction::ScreenshotMenu
            | KeyAction::Dictate,
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
            for head in &mut u.heads {
                if let Err(e) = head.compositor.reset_state() {
                    eprintln!("patina-compositor: {e}");
                }
                head.frame_pending = false;
                head.dirty = true;
            }
            u.active = true;
            // Gamma doesn't survive another session having the GPU.
            u.apply_gamma();
            state.needs_redraw = true;
            // Coming back took a key on another console, which we never
            // saw: that's input, and the screen comes on.
            state.input_activity();
        }
    }
}

fn vblank(state: &mut State, crtc: crtc::Handle) {
    let Some(u) = state.udev.as_mut() else { return };
    let Some(head) = u.heads.iter_mut().find(|h| h.crtc == crtc) else {
        return;
    };
    if let Err(e) = head.compositor.frame_submitted() {
        eprintln!("patina-compositor: {e:?}");
    }
    head.frame_pending = false;
}

/// Draws the monitors that changed and are ready for a frame.
pub fn schedule(state: &mut State) {
    let Some(u) = state.udev.as_mut() else { return };
    if u.off {
        // Nothing's shown, so a lock is as good as on screen, and sleep
        // needn't wait for it.
        state.lock_drawn();
        return;
    }
    if !u.active {
        return;
    }
    if state.needs_redraw {
        state.needs_redraw = false;
        for head in &mut u.heads {
            head.dirty = true;
        }
    }
    if u.heads.iter().any(|h| h.dirty && !h.frame_pending) {
        render(state);
    }
}

fn render(state: &mut State) {
    let locked = state.locked();
    let lock_surface = state.lock_surface().cloned();
    let dnd_icon = state.dnd_icon_at();
    let fullscreen = state.fullscreen_drawn();
    state.paint_frames();
    let ready: Vec<(usize, Output)> = match state.udev.as_ref() {
        Some(u) => u
            .heads
            .iter()
            .enumerate()
            .filter(|(_, h)| h.dirty && !h.frame_pending)
            .map(|(i, h)| (i, h.output.clone()))
            .collect(),
        None => return,
    };
    let pointer = state
        .seat
        .get_pointer()
        .map(|p| p.current_location())
        .unwrap_or_default();
    // The compositor's cursor during a grab, else the client's.
    let shown = match state.grab_cursor {
        Some(icon) => CursorImageStatus::Named(icon),
        None => state.cursor.clone(),
    };
    let time = state.start.elapsed();

    for (index, output) in ready {
        let view = state.view_of(&output);
        let (origin, scale) = (view.area.loc, view.scale);
        // A screenshot's flash, where a drop would tile (ADR-0049), and a
        // selection being made (ADR-0091).
        let flash: Vec<_> = state
            .flash_element(view)
            .into_iter()
            .chain(state.preview_element(view))
            .chain(state.selection_elements(view))
            .collect();
        let Some(u) = state.udev.as_mut() else { return };

        let mut elements: Vec<OutputElement> = Vec::new();
        elements.extend(crate::cursor::elements(
            &mut u.renderer,
            &mut u.cursors,
            &shown,
            pointer - origin.to_f64(),
            scale,
        ));
        let dnd_here = dnd_icon.as_ref().map(|(s, at)| (s.clone(), *at - origin.to_f64()));
        elements.extend(
            crate::dnd::elements(&mut u.renderer, dnd_here.as_ref(), scale)
                .into_iter()
                .map(OutputElement::Surface),
        );
        elements.extend(flash.into_iter().map(OutputElement::Flash));
        if locked {
            // Only the locker, over black, on the primary: the locker is
            // sized to it, and the others stay black.
            if index == 0 {
                elements.extend(
                    crate::lock::elements(&mut u.renderer, lock_surface.as_ref(), scale)
                        .into_iter()
                        .map(OutputElement::Surface),
                );
            }
        } else {
            // A fullscreen window over the bar, on the monitor it covers.
            let fullscreen_here = fullscreen
                .as_ref()
                .filter(|(_, at)| view.area.contains(*at))
                .map(|(w, at)| (w.clone(), *at - origin));
            elements.extend(
                crate::fullscreen::elements(&mut u.renderer, fullscreen_here.as_ref(), scale)
                    .into_iter()
                    .map(OutputElement::Surface),
            );
            elements.extend(
                crate::frames::desktop_elements(
                    &mut u.renderer,
                    &state.space,
                    &output,
                    state.fullscreen.as_ref(),
                    false,
                )
                .into_iter()
                .map(OutputElement::Desktop),
            );
        }

        let head = &mut u.heads[index];
        let queued = match head.compositor.render_frame(
            &mut u.renderer,
            &elements,
            BACKGROUND,
            FrameFlags::DEFAULT,
        ) {
            Ok(frame) if !frame.is_empty => match head.compositor.queue_frame(()) {
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
        head.frame_pending = true;
        head.dirty = false;
        if !queued {
            // No vblank is coming; hold the next frame back a little instead.
            let crtc = head.crtc;
            let _ = u
                .loop_handle
                .insert_source(Timer::from_duration(IDLE_FRAME), move |_, _, state| {
                    if let Some(h) = state
                        .udev
                        .as_mut()
                        .and_then(|u| u.heads.iter_mut().find(|h| h.crtc == crtc))
                    {
                        h.frame_pending = false;
                    }
                    TimeoutAction::Drop
                });
        }

        for window in state.space.elements() {
            let on_it = state
                .space
                .element_bbox(window)
                .is_some_and(|b| b.overlaps(view.area));
            if on_it {
                window.send_frame(&output, time, Some(Duration::ZERO), |_, _| {
                    Some(output.clone())
                });
            }
        }
        for layer in layer_map_for_output(&output).layers() {
            layer.send_frame(&output, time, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        if index == 0 {
            if let CursorImageStatus::Surface(surface) = &state.cursor {
                send_frames_surface_tree(surface, &output, time, Some(Duration::ZERO), |_, _| {
                    Some(output.clone())
                });
            }
            crate::lock::send_frames(lock_surface.as_ref(), &output, time);
            crate::dnd::send_frames(dnd_icon.as_ref(), &output, time);
        }
    }
    state.lock_drawn();
    state.space.refresh();
    state.popups.cleanup();
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
