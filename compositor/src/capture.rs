//! Screen capture for other programs (ADR-0092): ext-image-copy-capture-v1,
//! of the screen or of one window, with the windows listed for it by
//! ext-foreign-toplevel-list-v1. xdg-desktop-portal-wlr is the client that
//! matters: OBS, browsers sharing a screen and the like ask the portal,
//! which asks this, and streams what it gets over PipeWire.
//!
//! A frame a client asks for waits until what it shows has changed since
//! its last (its damage tracker says so), then the scene is drawn offscreen
//! with the backend's renderer, read back and copied into the client's shm
//! buffer. No dmabufs yet: a copy through memory, as screenshots make
//! (ADR-0034). The desktop's own recording (`record.rs`) draws the same
//! scene.

pub mod record;

use std::time::Duration;

use smithay::{
    backend::renderer::{
        Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
        damage::OutputDamageTracker,
        element::{
            AsRenderElements, memory::MemoryRenderBufferRenderElement, render_elements,
            surface::WaylandSurfaceRenderElement,
        },
    },
    desktop::{Space, Window},
    input::pointer::CursorImageStatus,
    output::Output,
    reexports::wayland_server::protocol::{wl_shm, wl_surface::WlSurface},
    utils::{
        Buffer as BufferCoords, Clock, IsAlive, Logical, Monotonic, Physical, Point, Rectangle,
        Size, Transform,
    },
    wayland::{
        foreign_toplevel_list::{
            ForeignToplevelHandle, ForeignToplevelListHandler, ForeignToplevelListState,
        },
        image_capture_source::{
            ImageCaptureSource, ImageCaptureSourceHandler, ImageCaptureSourceState,
            OutputCaptureSourceHandler, OutputCaptureSourceState, ToplevelCaptureSourceHandler,
            ToplevelCaptureSourceState,
        },
        image_copy_capture::{
            BufferConstraints, CaptureFailureReason, Frame, ImageCopyCaptureHandler,
            ImageCopyCaptureState, Session, SessionRef,
        },
        shm::with_buffer_contents_mut,
    },
};

use crate::state::State;

/// What a capture source shows.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    /// The whole screen, as it's shown.
    Screen,
    /// One window, with its frame if it has one, wherever it is.
    Window(WlSurface),
}

/// The formats a client's buffer may have, both B, G, R, A in memory:
/// without alpha first, as a screen has none, which is what capture clients
/// expect.
const SHM: [wl_shm::Format; 2] = [wl_shm::Format::Xrgb8888, wl_shm::Format::Argb8888];

/// What a session shows, looked up before drawing: the window if it's
/// one, whether that's framed, and the size now.
type Looked = (Target, Option<Window>, bool, Option<Size<i32, Physical>>);

/// Where a frame changed, or `None` for all of it.
type Damage = Option<Vec<Rectangle<i32, BufferCoords>>>;

/// A client's capture session, and the frames it's waiting for: usually
/// one, but xdg-desktop-portal-wlr can ask for a second before the first
/// is answered.
struct Watched {
    session: Session,
    /// What it was last sent, to tell whether there's anything new.
    tracker: Option<(Size<i32, Physical>, OutputDamageTracker)>,
    pending: Vec<Frame>,
    /// Whether it has had a frame yet: the first comes at once.
    sent: bool,
}

/// The capture protocols' state, and the sessions open.
pub struct Capture {
    _sources: ImageCaptureSourceState,
    _outputs: OutputCaptureSourceState,
    _windows: ToplevelCaptureSourceState,
    copy: ImageCopyCaptureState,
    /// ext-foreign-toplevel-list, which names the windows a client can
    /// capture (kept up to date by `toplevels::refresh`).
    pub toplevel_list: ForeignToplevelListState,
    sessions: Vec<Watched>,
    /// The desktop's own recording, if one is going (`record.rs`).
    pub recording: Option<record::Recording>,
}

impl Capture {
    pub fn new(dh: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        Self {
            _sources: ImageCaptureSourceState::new(),
            _outputs: OutputCaptureSourceState::new::<State>(dh),
            _windows: ToplevelCaptureSourceState::new::<State>(dh),
            copy: ImageCopyCaptureState::new::<State>(dh),
            toplevel_list: ForeignToplevelListState::new::<State>(dh),
            sessions: Vec::new(),
            recording: None,
        }
    }
}

render_elements! {
    /// What a capture draws: the pointer, surfaces over the desktop (the
    /// locker, a fullscreen window), then the desktop.
    pub CaptureElement<R> where R: ImportAll + ImportMem;
    Cursor=MemoryRenderBufferRenderElement<R>,
    Surface=WaylandSurfaceRenderElement<R>,
    Desktop=crate::frames::DesktopElement<R>,
}

/// What the screen shows now, for drawing it again offscreen: the space
/// and output borrowed, so the renderer beside them can be too.
pub struct Scene<'a> {
    pub space: &'a Space<Window>,
    pub output: &'a Output,
    pub shown: Shown,
}

/// The rest of what's on screen.
pub struct Shown {
    pub scale: f64,
    pub locked: bool,
    pub lock_surface: Option<WlSurface>,
    /// The window over everything, and where.
    pub fullscreen: Option<(Window, Point<i32, Logical>)>,
    pub fullscreen_window: Option<Window>,
    pub pointer: Point<f64, Logical>,
    pub cursor: CursorImageStatus,
}

impl Scene<'_> {
    /// The screen's elements, top first: the pointer if `cursors` draws
    /// it (on the hardware; nested, the parent desktop draws it), then as
    /// the backends draw it. `shot` leaves out the screenshot chooser and
    /// the recording's indicator.
    pub fn screen<R>(
        &self,
        renderer: &mut R,
        cursors: Option<&mut crate::cursor::Cursors>,
        shot: bool,
    ) -> Vec<CaptureElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let mut elements = Vec::new();
        if let Some(cursors) = cursors {
            elements.extend(crate::cursor::elements::<R, CaptureElement<R>>(
                renderer,
                cursors,
                &self.shown.cursor,
                self.shown.pointer,
                self.shown.scale,
            ));
        }
        if self.shown.locked {
            // Only the locker, as on screen.
            elements.extend(
                crate::lock::elements(renderer, self.shown.lock_surface.as_ref(), self.shown.scale)
                    .into_iter()
                    .map(CaptureElement::Surface),
            );
            return elements;
        }
        elements.extend(
            crate::fullscreen::elements(renderer, self.shown.fullscreen.as_ref(), self.shown.scale)
                .into_iter()
                .map(CaptureElement::Surface),
        );
        elements.extend(
            crate::frames::desktop_elements(
                renderer,
                self.space,
                self.output,
                self.shown.fullscreen_window.as_ref(),
                shot,
            )
            .into_iter()
            .map(CaptureElement::Desktop),
        );
        elements
    }

    /// `window` alone at the top left, with its frame if `framed`, and its
    /// size in pixels; as a screenshot of it takes it.
    fn window<R>(
        &self,
        renderer: &mut R,
        window: &Window,
        framed: bool,
    ) -> (Vec<CaptureElement<R>>, Size<i32, Physical>)
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + Texture + 'static,
    {
        let (size, at) = window_layout(window, framed, self.shown.scale);
        let mut elements: Vec<CaptureElement<R>> = window
            .render_elements::<WaylandSurfaceRenderElement<R>>(
                renderer,
                at,
                self.shown.scale.into(),
                1.0,
            )
            .into_iter()
            .map(CaptureElement::Surface)
            .collect();
        if framed {
            elements.extend(
                crate::frames::screenshot_element(renderer, window, self.shown.scale).map(
                    |frame| CaptureElement::Desktop(crate::frames::DesktopElement::Frame(frame)),
                ),
            );
        }
        (elements, size)
    }
}

/// A window's size in pixels with its frame (if `framed`), and where its
/// surface goes in that: its own edges, without a client's shadow.
fn window_layout(
    window: &Window,
    framed: bool,
    scale: f64,
) -> (Size<i32, Physical>, Point<i32, Physical>) {
    let geometry = window.geometry();
    let top = if framed { crate::frames::TITLE } else { 0 };
    let size = Size::<i32, Logical>::from((geometry.size.w, geometry.size.h + top))
        .to_physical_precise_round(scale);
    let at =
        (Point::<i32, Logical>::from((0, top)) - geometry.loc).to_physical_precise_round(scale);
    (size, at)
}

/// Copies `rgba` (premultiplied, `size`, rows from the top) into an shm
/// buffer of `SHM`'s formats, which are B, G, R, A in memory.
fn copy_into(
    buffer: &smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer,
    rgba: &[u8],
    size: Size<i32, Physical>,
) -> Result<(), String> {
    with_buffer_contents_mut(buffer, |ptr, len, data| {
        let (w, h) = (size.w as usize, size.h as usize);
        let stride = data.stride as usize;
        let offset = data.offset as usize;
        if (data.width as usize) < w
            || (data.height as usize) < h
            || offset + stride * (h.saturating_sub(1)) + w * 4 > len
        {
            return Err("the buffer is too small".to_string());
        }
        // SAFETY: the pool is mapped for `len` bytes, checked above.
        let memory = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
        for y in 0..h {
            let row = &mut memory[offset + y * stride..offset + y * stride + w * 4];
            let from = &rgba[y * w * 4..(y + 1) * w * 4];
            to_bgra(row, from);
        }
        Ok(())
    })
    .map_err(|e| format!("{e:?}"))?
}

/// RGBA pixels into `to` as B, G, R, A.
fn to_bgra(to: &mut [u8], rgba: &[u8]) {
    for (to, from) in to.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
        to.copy_from_slice(&[from[2], from[1], from[0], from[3]]);
    }
}

impl State {
    /// What's on screen now, but the space and output.
    pub fn shown(&self) -> Shown {
        Shown {
            scale: self.output.current_scale().fractional_scale(),
            locked: self.locked(),
            lock_surface: self.lock_surface().cloned(),
            fullscreen: self.fullscreen_drawn(),
            fullscreen_window: self.fullscreen.clone(),
            pointer: self
                .seat
                .get_pointer()
                .map(|p| p.current_location())
                .unwrap_or_default(),
            cursor: match self.grab_cursor {
                Some(icon) => CursorImageStatus::Named(icon),
                None => self.cursor.clone(),
            },
        }
    }

    fn target(&self, source: &ImageCaptureSource) -> Option<Target> {
        source.user_data().get::<Target>().cloned()
    }

    /// The window `target` names, if it's shown; `None` for the screen.
    fn target_window(&self, target: &Target) -> Option<Window> {
        match target {
            Target::Screen => None,
            Target::Window(surface) => self
                .window_for(surface)
                .filter(|w| self.space.elements().any(|e| e == w)),
        }
    }

    /// The size a capture of `target` is now, in pixels.
    fn target_size(&self, target: &Target) -> Option<Size<i32, Physical>> {
        let scale = self.output.current_scale().fractional_scale();
        match target {
            Target::Screen => Some(self.output.current_mode()?.size),
            Target::Window(_) => {
                let window = self.target_window(target)?;
                Some(window_layout(&window, self.framed(&window), scale).0)
            }
        }
    }

    /// Answers the frames clients are waiting for that have something new,
    /// and records a frame if a recording is going. Called after each pass
    /// of the event loop.
    pub fn capture_frames(&mut self) {
        self.capture.sessions.retain(|w| w.session.alive());
        // A window closed: its sessions end (a session dropped says it's
        // stopped).
        let closed: Vec<usize> = self
            .capture
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, w)| match self.target(&w.session.source()) {
                Some(Target::Window(surface)) => self.window_for(&surface).is_none(),
                _ => false,
            })
            .map(|(i, _)| i)
            .collect();
        for i in closed.into_iter().rev() {
            self.capture.sessions.remove(i);
        }
        let waiting = self.capture.sessions.iter().any(|w| !w.pending.is_empty());
        if waiting {
            self.answer_frames();
        }
        self.record_frame();
    }

    fn answer_frames(&mut self) {
        // What each session shows, while the windows can be looked at.
        let targets: Vec<Option<Looked>> = self
            .capture
            .sessions
            .iter()
            .map(|w| {
                let target = self.target(&w.session.source())?;
                let window = self.target_window(&target);
                let framed = window.as_ref().is_some_and(|w| self.framed(w));
                let size = self.target_size(&target);
                Some((target, window, framed, size))
            })
            .collect();
        let now: Duration = Clock::<Monotonic>::new().now().into();
        let scene = Scene {
            space: &self.space,
            output: &self.output,
            shown: self.shown(),
        };
        let sessions = &mut self.capture.sessions;
        for (watched, target) in sessions.iter_mut().zip(targets) {
            if watched.pending.is_empty() {
                continue;
            }
            let frames = std::mem::take(&mut watched.pending);
            let fail = |frames: Vec<Frame>, reason| frames.into_iter().for_each(|f| f.fail(reason));
            let (target, window, framed, size) = match target {
                Some((target, window, framed, Some(size))) => (target, window, framed, size),
                // A minimised window: its frames wait until it's back.
                Some((Target::Window(_), None, ..)) => {
                    watched.pending = frames;
                    continue;
                }
                _ => {
                    fail(frames, CaptureFailureReason::Stopped);
                    continue;
                }
            };
            let wanted = watched.session.current_constraints().map(|c| c.size);
            let buffer_size = Size::<i32, BufferCoords>::from((size.w, size.h));
            if wanted != Some(buffer_size) {
                watched.session.update_constraints(constraints(buffer_size));
                watched.tracker = None;
                fail(frames, CaptureFailureReason::BufferConstraints);
                continue;
            }
            let draw_cursor = watched.session.draw_cursor();
            let result = if let Some(u) = self.udev.as_mut() {
                let cursors = draw_cursor.then_some(&mut u.cursors);
                answer::<_, smithay::backend::renderer::gles::GlesTexture>(
                    &mut u.renderer,
                    &scene,
                    cursors,
                    &target,
                    window.as_ref(),
                    framed,
                    size,
                    watched,
                    &frames,
                )
            } else if let Some(w) = self.winit.as_mut() {
                answer::<_, smithay::backend::renderer::gles::GlesTexture>(
                    w.backend.renderer(),
                    &scene,
                    None,
                    &target,
                    window.as_ref(),
                    framed,
                    size,
                    watched,
                    &frames,
                )
            } else if let Some(n) = self.nested.as_mut() {
                answer::<_, smithay::reexports::pixman::Image<'static, 'static>>(
                    &mut n.renderer,
                    &scene,
                    None,
                    &target,
                    window.as_ref(),
                    framed,
                    size,
                    watched,
                    &frames,
                )
            } else {
                Err("no renderer".to_string())
            };
            match result {
                Ok(Some(damage)) => {
                    watched.sent = true;
                    for frame in frames {
                        frame.success(Transform::Normal, damage.clone(), now);
                    }
                }
                // Nothing new: they wait for something to change.
                Ok(None) => watched.pending = frames,
                Err(e) => {
                    eprintln!("patina-compositor: capture: {e}");
                    fail(frames, CaptureFailureReason::Unknown);
                }
            }
        }
    }
}

/// Draws `target` into each of `frames`' buffers if it has changed since
/// the session's last frame (or it has had none): `Ok(Some(damage))` once
/// it's copied, `Ok(None)` if there's nothing new.
#[allow(clippy::too_many_arguments)]
fn answer<R, T>(
    renderer: &mut R,
    scene: &Scene<'_>,
    cursors: Option<&mut crate::cursor::Cursors>,
    target: &Target,
    window: Option<&Window>,
    framed: bool,
    size: Size<i32, Physical>,
    watched: &mut Watched,
    frames: &[Frame],
) -> Result<Option<Damage>, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let (elements, clear) = match (target, window) {
        (Target::Screen, _) => (
            scene.screen(renderer, cursors, false),
            crate::nested::BACKGROUND,
        ),
        (Target::Window(_), Some(window)) => (scene.window(renderer, window, framed).0, [0.0; 4]),
        (Target::Window(_), None) => return Err("the window is gone".into()),
    };
    let tracker = match &mut watched.tracker {
        Some((at, tracker)) if *at == size => tracker,
        slot => {
            &mut slot
                .insert((
                    size,
                    OutputDamageTracker::new(size, scene.shown.scale, Transform::Normal),
                ))
                .1
        }
    };
    let damage = tracker
        .damage_output(1, &elements)
        .map_err(|e| format!("{e:?}"))?
        .0
        .cloned();
    if damage.is_none() && watched.sent {
        return Ok(None);
    }
    let rgba = crate::screenshot::read_back::<R, T, _>(
        renderer,
        &elements,
        size,
        scene.shown.scale,
        clear,
    )?;
    for frame in frames {
        copy_into(&frame.buffer(), &rgba, size)?;
    }
    let damage = damage.filter(|_| watched.sent).map(|rects| {
        rects
            .into_iter()
            .map(|r| Rectangle::new((r.loc.x, r.loc.y).into(), (r.size.w, r.size.h).into()))
            .collect()
    });
    Ok(Some(damage))
}

fn constraints(size: Size<i32, BufferCoords>) -> BufferConstraints {
    BufferConstraints {
        size,
        shm: SHM.to_vec(),
        dma: None,
    }
}

impl ForeignToplevelListHandler for State {
    fn foreign_toplevel_list_state(&mut self) -> &mut ForeignToplevelListState {
        &mut self.capture.toplevel_list
    }
}

impl ImageCaptureSourceHandler for State {}

impl OutputCaptureSourceHandler for State {
    fn output_capture_source_state(&mut self) -> &mut OutputCaptureSourceState {
        &mut self.capture._outputs
    }

    fn output_source_created(&mut self, source: ImageCaptureSource, _output: &Output) {
        source.user_data().insert_if_missing(|| Target::Screen);
    }
}

impl ToplevelCaptureSourceHandler for State {
    fn toplevel_capture_source_state(&mut self) -> &mut ToplevelCaptureSourceState {
        &mut self.capture._windows
    }

    fn toplevel_source_created(
        &mut self,
        source: ImageCaptureSource,
        toplevel: ForeignToplevelHandle,
    ) {
        if let Some(surface) = self.toplevels.surface_of(&toplevel) {
            source
                .user_data()
                .insert_if_missing(|| Target::Window(surface));
        }
    }
}

impl ImageCopyCaptureHandler for State {
    fn image_copy_capture_state(&mut self) -> &mut ImageCopyCaptureState {
        &mut self.capture.copy
    }

    fn capture_constraints(&mut self, source: &ImageCaptureSource) -> Option<BufferConstraints> {
        let target = self.target(source)?;
        let size = self.target_size(&target)?;
        Some(constraints((size.w, size.h).into()))
    }

    fn new_session(&mut self, session: Session) {
        self.capture.sessions.push(Watched {
            session,
            tracker: None,
            pending: Vec::new(),
            sent: false,
        });
    }

    fn frame(&mut self, session: &SessionRef, frame: Frame) {
        if let Some(watched) = self
            .capture
            .sessions
            .iter_mut()
            .find(|w| w.session == *session)
        {
            watched.pending.push(frame);
        }
    }

    fn frame_aborted(&mut self, frame: smithay::wayland::image_copy_capture::FrameRef) {
        for watched in &mut self.capture.sessions {
            watched.pending.retain(|f| *f != frame);
        }
    }

    fn session_destroyed(&mut self, session: SessionRef) {
        self.capture.sessions.retain(|w| w.session != session);
    }
}

smithay::delegate_foreign_toplevel_list!(State);
smithay::delegate_image_capture_source!(State);
smithay::delegate_output_capture_source!(State);
smithay::delegate_toplevel_capture_source!(State);
smithay::delegate_image_copy_capture!(State);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_go_into_the_buffer_as_bgra() {
        let mut row = [0u8; 8];
        to_bgra(&mut row, &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(row, [3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
