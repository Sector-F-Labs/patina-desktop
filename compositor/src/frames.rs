//! Frames for windows that draw none: X11 windows (ADR-0077), as the
//! Minecraft launcher, a managed window that isn't fullscreen and hasn't set
//! `_MOTIF_WM_HINTS` without decorations; and Wayland windows that take
//! xdg-decoration's server side (ADR-0085), as foot and SDL's games. Each
//! gets a title bar above it. The bar
//! is patina-ui's own `TitleBar`, the one Patina apps draw, run in an egui
//! context of its own per window and painted on the CPU with patina-shell's
//! painter, in the bar's skin; its buttons and drags come back as viewport
//! commands, which the compositor carries out as it would for the app.
//!
//! A window's place in the space stays its own surface's; the bar sits
//! above it, `TITLE` high. Patina's one-pixel border goes round both, as an
//! app's own frame has it: painted round the bar's top, and drawn as lines
//! over the window's sides and bottom. Placing a window takes the rectangle with the
//! bar (`outer`) and gives the window the rest (`inner`).

use std::cell::RefCell;
use std::time::SystemTime;

use egui::{Pos2, ViewportCommand, ViewportId};
use patina::frame::{TitleBar, corner_radius, header_frame, release_grab};
use patina::skin::Skin;
use patina_shell::raster::Painter;
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportAll, ImportMem, Renderer, Texture,
            element::{
                AsRenderElements, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::WaylandSurfaceRenderElement,
            },
        },
    },
    desktop::{Space, Window, layer_map_for_output},
    input::pointer::{CursorIcon, Focus, GrabStartData},
    output::Output,
    reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    render_elements,
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER, Scale, Size, Transform},
    wayland::shell::{
        wlr_layer::Layer,
        xdg::{ToplevelSurface, decoration::XdgDecorationHandler},
    },
    xwayland::xwm::WmWindowType,
};

use crate::grabs::{BTN_LEFT, Edges, ResizeGrab};
use crate::state::State;

/// The space above and below the title bar's row. Patina apps' headers have
/// 8, as they hold tabs, menus and buttons; a bare title bar looks too tall
/// with it.
const PADDING: i8 = 4;

/// The title bar's height: patina-ui's 25-point row of window buttons with
/// `PADDING` above and below.
pub const TITLE: i32 = 25 + 2 * PADDING as i32;

/// How far outside the window, and into the title bar's top, a press
/// resizes it.
const EDGE: i32 = 5;
/// Along an edge, how near a corner resizes both ways.
const CORNER: i32 = 12;

/// Whether `window` gets a frame drawn, as long as it isn't fullscreen: a
/// Wayland window in xdg-decoration's server side, or an X11 window the
/// window manager handles that hasn't said it draws its own, and is a
/// window rather than a splash, menu or toolbar.
pub fn wants_frame(window: &Window) -> bool {
    if let Some(toplevel) = window.toplevel() {
        return toplevel.current_state().decoration_mode == Some(Mode::ServerSide);
    }
    let Some(x11) = window.x11_surface() else {
        return false;
    };
    !x11.is_override_redirect()
        && !x11.is_decorated()
        && matches!(
            x11.window_type(),
            None | Some(WmWindowType::Normal | WmWindowType::Dialog)
        )
}

/// How far above its surface `window`'s frame reaches: the title bar's
/// height, or 0 without one.
pub fn top(window: &Window) -> i32 {
    if wants_frame(window) { TITLE } else { 0 }
}

/// The part of `outer`, a rectangle with the frame, that `window` itself
/// gets.
pub fn inner(window: &Window, outer: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let top = top(window);
    Rectangle::new(
        (outer.loc.x, outer.loc.y + top).into(),
        (outer.size.w, (outer.size.h - top).max(1)).into(),
    )
}

/// `rect`, `window`'s own, with its frame.
pub fn outer(window: &Window, rect: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let top = top(window);
    Rectangle::new(
        (rect.loc.x, rect.loc.y - top).into(),
        (rect.size.w, rect.size.h + top).into(),
    )
}

/// Where on a frame the pointer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The title bar: its buttons, or the rest, which moves the window.
    Title,
    /// An edge or corner, which resizes it.
    Edge(Edges),
}

/// The part of a frame around `rect` (a window's own) at `pos`, if it's on
/// one: within `EDGE` outside it or of the title bar's top, or on the bar.
fn part_at(rect: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Option<Part> {
    let (x, y) = (pos.x.floor() as i32, pos.y.floor() as i32);
    let (left, right) = (rect.loc.x, rect.loc.x + rect.size.w);
    let (top, bottom) = (rect.loc.y - TITLE, rect.loc.y + rect.size.h);
    if x < left - EDGE || x >= right + EDGE || y < top - EDGE || y >= bottom + EDGE {
        return None;
    }
    let on_bar = y >= top && y < rect.loc.y && x >= left && x < right;
    if x >= left && x < right && y >= rect.loc.y && y < bottom {
        return None;
    }
    let mut edges = Edges {
        top: y < top + EDGE.min(3),
        bottom: y >= bottom,
        left: x < left,
        right: x >= right,
    };
    if !(edges.top || edges.bottom || edges.left || edges.right) {
        return on_bar.then_some(Part::Title);
    }
    // Near a corner, both ways.
    if edges.top || edges.bottom {
        edges.left |= x < left + CORNER;
        edges.right |= x >= right - CORNER;
    }
    if edges.left || edges.right {
        edges.top |= y < top + CORNER;
        edges.bottom |= y >= bottom - CORNER;
    }
    Some(Part::Edge(edges))
}

/// What a frame is painted for; painted again when it changes.
#[derive(Debug, Clone, PartialEq)]
struct Look {
    width: i32,
    scale: f64,
    title: String,
    maximized: bool,
    skin: String,
}

/// A window's frame: its egui context and painter, and the last picture.
#[derive(Default)]
struct Frame {
    ctx: egui::Context,
    painter: Painter,
    pixels: Vec<u32>,
    buffer: Option<MemoryRenderBuffer>,
    look: Option<Look>,
    skin: Option<String>,
    /// egui asked to be run again soon (a hover fading in).
    again: bool,
    /// The border's colour, premultiplied; `None` while maximised.
    border: Option<[f32; 4]>,
    /// The border down the window's left and right, and along its bottom.
    sides: [SolidColorBuffer; 3],
}

#[derive(Default)]
struct FrameSlot(RefCell<Frame>);

fn slot(window: &Window) -> &FrameSlot {
    window.user_data().insert_if_missing(FrameSlot::default);
    window
        .user_data()
        .get::<FrameSlot>()
        .expect("inserted above")
}

impl Frame {
    /// Runs the title bar with `events`, paints it for `look`, and returns
    /// what its buttons and drags asked for.
    fn run(&mut self, look: &Look, skin: &Skin, events: Vec<egui::Event>) -> Vec<ViewportCommand> {
        if self.skin.as_deref() != Some(skin.name.as_str()) {
            skin.apply(&self.ctx);
            self.skin = Some(skin.name.clone());
        }
        let size = egui::vec2(look.width as f32, TITLE as f32);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
            events,
            focused: true,
            ..Default::default()
        };
        // After the bar handed a drag to the compositor's move, the button
        // came up under the grab: tell egui, as an app's input hook does,
        // or it thinks the bar's still held and the next drag never starts.
        release_grab(&self.ctx, &mut input);
        let viewport = input.viewports.entry(ViewportId::ROOT).or_default();
        viewport.native_pixels_per_point = Some(look.scale as f32);
        viewport.maximized = Some(look.maximized);
        let mut border = None;
        let output = self.ctx.run_ui(input, |ui| {
            let frame = skin.frame_style(ui.visuals());
            // The skin's, as an app's own frame has: square in a skin that
            // squares its windows.
            let radius = corner_radius(ui.ctx(), frame.radius);
            egui::CentralPanel::default()
                .frame(
                    header_frame(ui.style(), &frame, radius)
                        .inner_margin(egui::Margin::symmetric(12, PADDING)),
                )
                .show(ui, |ui| {
                    TitleBar::new(&look.title, &frame).show(ui, |_| {}, |_| {});
                });
            // The window's border, as `patina::frame::edges` draws it, of
            // which this is the top: the rest lies below the bar.
            if !look.maximized {
                let window = egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(size.x, size.y + 2.0 * radius as f32),
                );
                ui.ctx()
                    .layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("window_border"),
                    ))
                    .rect_stroke(
                        window,
                        radius,
                        egui::Stroke::new(1.0, frame.border),
                        egui::StrokeKind::Inside,
                    );
            }
            border = (!look.maximized).then(|| frame.border.to_normalized_gamma_f32());
        });
        self.border = border;
        let ppp = output.pixels_per_point;
        let primitives = self.ctx.tessellate(output.shapes, ppp);
        let w = (look.width as f64 * look.scale).round().max(1.0) as usize;
        let h = (TITLE as f64 * look.scale).round() as usize;
        self.pixels.resize(w * h, 0);
        self.painter.paint(
            &mut self.pixels,
            w,
            h,
            ppp,
            &primitives,
            &output.textures_delta,
        );
        let bytes: Vec<u8> = self.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        self.buffer = Some(MemoryRenderBuffer::from_slice(
            &bytes,
            Fourcc::Argb8888,
            (w as i32, h as i32),
            1,
            Transform::Normal,
            None,
        ));
        self.look = Some(look.clone());
        let viewport = output.viewport_output.get(&ViewportId::ROOT);
        self.again = viewport.is_some_and(|v| v.repaint_delay.as_millis() < 500);
        viewport.map(|v| v.commands.clone()).unwrap_or_default()
    }
}

/// The frame of `window`, whose own surface is at `location` (relative to
/// the output), to draw: `None` if it hasn't been painted.
fn frame_element<R>(
    renderer: &mut R,
    window: &Window,
    location: Point<i32, Logical>,
    scale: f64,
) -> Option<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    let frame = slot(window).0.borrow();
    let (buffer, look) = (frame.buffer.as_ref()?, frame.look.as_ref()?);
    let at = Point::<i32, Logical>::from((location.x, location.y - TITLE));
    let pixels = buffer_size(look);
    MemoryRenderBufferRenderElement::from_buffer(
        renderer,
        at.to_f64()
            .to_physical(scale)
            .to_i32_round::<i32>()
            .to_f64(),
        buffer,
        None,
        Some(Rectangle::from_size(
            (pixels.w as f64, pixels.h as f64).into(),
        )),
        Some((look.width, TITLE).into()),
        Kind::Unspecified,
    )
    .ok()
}

fn buffer_size(look: &Look) -> Size<i32, Logical> {
    (
        (look.width as f64 * look.scale).round().max(1.0) as i32,
        (TITLE as f64 * look.scale).round() as i32,
    )
        .into()
}

/// The border down the sides and along the bottom of `window`, whose own
/// surface is at `location`: one pixel inside its edges, over it.
fn border_elements<R>(
    window: &Window,
    location: Point<i32, Logical>,
    scale: f64,
) -> Vec<DesktopElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let mut frame = slot(window).0.borrow_mut();
    let Some(color) = frame.border else {
        return Vec::new();
    };
    let size = window.geometry().size;
    let (w, h) = (size.w.max(1), size.h.max(1));
    let lines: [(Point<i32, Logical>, Size<i32, Logical>); 3] = [
        ((0, 0).into(), (1, h).into()),
        ((w - 1, 0).into(), (1, h).into()),
        ((0, h - 1).into(), (w, 1).into()),
    ];
    frame
        .sides
        .iter_mut()
        .zip(lines)
        .map(|(buffer, (at, size))| {
            buffer.update(size, color);
            DesktopElement::Border(SolidColorRenderElement::from_buffer(
                buffer,
                (location + at).to_physical_precise_round(scale),
                scale,
                1.0,
                Kind::Unspecified,
            ))
        })
        .collect()
}

render_elements! {
    /// The desktop: layer surfaces, windows and their frames.
    pub DesktopElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Frame=MemoryRenderBufferRenderElement<R>,
    Border=SolidColorRenderElement,
}

/// The desktop on `output`, top first, as Smithay's `space_render_elements`
/// draws it (the top and overlay layers, the windows, then the bottom and
/// background layers), with each framed window's frame beside it in the
/// stacking order; `fullscreen` is drawn without one. For a `shot`, the
/// screenshot chooser is left out (ADR-0091), in case it's still mapped.
pub fn desktop_elements<R>(
    renderer: &mut R,
    space: &Space<Window>,
    output: &Output,
    fullscreen: Option<&Window>,
    shot: bool,
) -> Vec<DesktopElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let scale = output.current_scale().fractional_scale();
    let mut elements = Vec::new();
    let layer_map = layer_map_for_output(output);
    let (lower, upper): (Vec<_>, Vec<_>) = layer_map
        .layers()
        .rev()
        .filter(|s| !(shot && s.namespace() == crate::screenshot::menu::NAMESPACE))
        .partition(|s| matches!(s.layer(), Layer::Background | Layer::Bottom));
    let layers = |renderer: &mut R, surfaces: Vec<_>, elements: &mut Vec<DesktopElement<R>>| {
        for surface in surfaces {
            let Some(geometry) = layer_map.layer_geometry(surface) else {
                continue;
            };
            elements.extend(
                AsRenderElements::<R>::render_elements::<WaylandSurfaceRenderElement<R>>(
                    surface,
                    renderer,
                    geometry.loc.to_physical_precise_round(scale),
                    Scale::from(scale),
                    1.0,
                )
                .into_iter()
                .map(DesktopElement::Surface),
            );
        }
    };
    layers(renderer, upper, &mut elements);
    if let Some(screen) = space.output_geometry(output) {
        for window in space.elements().rev() {
            let Some(location) = space.element_location(window) else {
                continue;
            };
            let location = location - screen.loc;
            let at = (location - window.geometry().loc).to_physical_precise_round(scale);
            // The frame first, so its border lies over the window's edges.
            if wants_frame(window) && fullscreen != Some(window) {
                elements.extend(border_elements(window, location, scale));
                elements.extend(
                    frame_element(renderer, window, location, scale).map(DesktopElement::Frame),
                );
            }
            elements.extend(
                window
                    .render_elements::<WaylandSurfaceRenderElement<R>>(
                        renderer,
                        at,
                        Scale::from(scale),
                        1.0,
                    )
                    .into_iter()
                    .map(DesktopElement::Surface),
            );
        }
    }
    layers(renderer, lower, &mut elements);
    elements
}

/// `window`'s frame alone, at the top left, for a screenshot of it.
pub fn screenshot_element<R>(
    renderer: &mut R,
    window: &Window,
    scale: f64,
) -> Option<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + 'static,
{
    if !wants_frame(window) {
        return None;
    }
    frame_element(renderer, window, (0, TITLE).into(), scale)
}

/// The frames' skin, and the pointer on them.
pub struct Frames {
    skin: Skin,
    /// The bar's config's mtime when the skin was read.
    skin_read: Option<SystemTime>,
    /// The window whose title bar the pointer is over.
    hovered: Option<Window>,
    /// The window whose title bar has the left button held on it.
    pressed: Option<Window>,
    /// Whether the cursor is the frames' (an edge's arrows).
    cursor: bool,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            skin: bar_skin(),
            skin_read: bar_mtime(),
            hovered: None,
            pressed: None,
            cursor: false,
        }
    }
}

/// The bar's skin, from its config file, as the bar reads it; not
/// `PATINA_SKIN`, which the bar gives the pieces it opens, and which a
/// compositor started from one of them (a terminal) would keep.
fn bar_skin() -> Skin {
    let name = patina_shell::bar::config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| patina_shell::bar::top_level(&text, "skin"));
    let Some(name) = name else {
        return Skin::default_skin();
    };
    let dirs: Vec<_> = patina::skin::app_dir("patina-bar").into_iter().collect();
    let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &dirs);
    skins
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(Skin::default_skin)
}

fn bar_mtime() -> Option<SystemTime> {
    std::fs::metadata(patina_shell::bar::config_path()?)
        .and_then(|m| m.modified())
        .ok()
}

impl State {
    /// Whether `window` has its frame drawn now: it wants one and isn't
    /// fullscreen.
    pub fn framed(&self, window: &Window) -> bool {
        wants_frame(window) && self.fullscreen.as_ref() != Some(window)
    }

    /// `window`'s rectangle with its frame, if it's mapped.
    pub fn outer_rect(&self, window: &Window) -> Option<Rectangle<i32, Logical>> {
        let rect = self.space.element_geometry(window)?;
        Some(if self.framed(window) {
            outer(window, rect)
        } else {
            rect
        })
    }

    fn look(&self, window: &Window) -> Option<Look> {
        Some(Look {
            width: window.geometry().size.w.max(1),
            scale: self.output.current_scale().fractional_scale(),
            title: title(window),
            maximized: crate::maximize::is_maximized(window),
            skin: self.frames.skin.name.clone(),
        })
    }

    /// Paints the frames that changed; call before drawing. Asks for
    /// another frame while one is fading.
    pub fn paint_frames(&mut self) {
        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|w| self.framed(w))
            .cloned()
            .collect();
        for window in windows {
            let Some(look) = self.look(&window) else {
                continue;
            };
            let mut frame = slot(&window).0.borrow_mut();
            if frame.look.as_ref() == Some(&look) && !frame.again {
                continue;
            }
            let commands = frame.run(&look, &self.frames.skin, Vec::new());
            if frame.again {
                self.needs_redraw = true;
            }
            drop(frame);
            self.frame_commands(&window, commands);
        }
    }

    /// Reads the skin again if the bar's config changed, so the frames
    /// follow it as the bar does, and the pinned apps, so the tab keys
    /// follow its tabs (ADR-0080).
    pub fn poll_bar_config(&mut self) {
        let mtime = bar_mtime();
        if mtime != self.frames.skin_read {
            self.frames.skin_read = mtime;
            self.frames.skin = bar_skin();
            self.toplevels.pinned = crate::toplevels::bar_pins();
            self.needs_redraw = true;
        }
    }

    /// The frame part under `pos`, and its window: not under the bar or a
    /// launcher, a fullscreen window, or another window on top.
    pub fn frame_under(&self, pos: Point<f64, Logical>) -> Option<(Window, Part)> {
        if self.locked() || self.fullscreen_on_top().is_some() {
            return None;
        }
        let map = layer_map_for_output(&self.output);
        if map.layer_under(Layer::Overlay, pos).is_some()
            || map.layer_under(Layer::Top, pos).is_some()
        {
            return None;
        }
        for window in self.space.elements().rev() {
            let Some(rect) = self.space.element_geometry(window) else {
                continue;
            };
            if self.framed(window)
                && let Some(part) = part_at(rect, pos)
            {
                return Some((window.clone(), part));
            }
            if self
                .space
                .element_bbox(window)
                .is_some_and(|b| b.to_f64().contains(pos))
            {
                return None;
            }
        }
        None
    }

    /// The pointer moved to `pos` with nothing grabbed: tells the title bar
    /// it's over, or held on, and shows an edge's arrows. True if it's on
    /// a frame, so no window gets it.
    pub fn frame_pointer(&mut self, pos: Point<f64, Logical>) -> bool {
        let under = self.frame_under(pos);
        let hovered = match &under {
            Some((window, Part::Title)) => Some(window.clone()),
            _ => None,
        };
        if self.frames.hovered != hovered
            && let Some(old) = self.frames.hovered.take()
            && self.frames.pressed.as_ref() != Some(&old)
        {
            self.frame_input(&old, vec![egui::Event::PointerGone]);
        }
        self.frames.hovered = hovered;
        // Held on a title bar, it follows the pointer anywhere, so a drag
        // can start.
        let target = self
            .frames
            .pressed
            .clone()
            .or_else(|| self.frames.hovered.clone());
        if let Some(window) = target
            && let Some(at) = self.on_frame(&window, pos)
        {
            self.frame_input(&window, vec![egui::Event::PointerMoved(at)]);
        }
        match under {
            Some((_, part)) => {
                self.grab_cursor = Some(match part {
                    Part::Title => CursorIcon::Default,
                    Part::Edge(edges) => edges.cursor(),
                });
                self.frames.cursor = true;
                self.needs_redraw = true;
                true
            }
            None => {
                if std::mem::take(&mut self.frames.cursor) {
                    self.grab_cursor = None;
                    self.needs_redraw = true;
                }
                self.frames.pressed.is_some()
            }
        }
    }

    /// A button pressed or let go at `pos`, with nothing grabbed: true if a
    /// frame took it. A press on a frame focuses its window; on an edge it
    /// starts a resize, on the title bar it goes to the bar.
    pub fn frame_button(&mut self, button: u32, pressed: bool, pos: Point<f64, Logical>) -> bool {
        if !pressed {
            let Some(window) = self.frames.pressed.take() else {
                return false;
            };
            if let Some(at) = self.on_frame(&window, pos) {
                self.frame_input(&window, vec![click(at, false)]);
            }
            return true;
        }
        let Some((window, part)) = self.frame_under(pos) else {
            return false;
        };
        self.focus_window(window.clone());
        if button != BTN_LEFT {
            return true;
        }
        match part {
            Part::Title => {
                self.frames.pressed = Some(window.clone());
                if let Some(at) = self.on_frame(&window, pos) {
                    self.frame_input(&window, vec![click(at, true)]);
                }
            }
            Part::Edge(edges) => {
                let start = GrabStartData {
                    focus: None,
                    button,
                    location: pos,
                };
                self.start_frame_resize(&window, edges, start);
            }
        }
        true
    }

    /// Resizes `window` from `edges` of its frame as the pointer moves, as
    /// its app would ask.
    fn start_frame_resize(&mut self, window: &Window, edges: Edges, start: GrabStartData<State>) {
        if window.x11_surface().is_some() {
            self.start_x11_resize(window, edges, start);
            return;
        }
        let (Some(location), Some(pointer)) =
            (self.space.element_location(window), self.seat.get_pointer())
        else {
            return;
        };
        let initial = Rectangle::new(location, window.geometry().size);
        if let Some(grab) = ResizeGrab::start(start, window.clone(), edges, initial) {
            self.grab_cursor = Some(edges.cursor());
            pointer.set_grab(self, grab, SERIAL_COUNTER.next_serial(), Focus::Clear);
        }
    }

    /// Gives `toplevel` the frame `mode` says: drawn here on the server
    /// side, or its own.
    fn decorate(&mut self, toplevel: &ToplevelSurface, mode: Mode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(mode));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    /// `pos` in the coordinates of `window`'s title bar.
    fn on_frame(&self, window: &Window, pos: Point<f64, Logical>) -> Option<Pos2> {
        let rect = self.space.element_geometry(window)?;
        Some(Pos2::new(
            (pos.x - rect.loc.x as f64) as f32,
            (pos.y - (rect.loc.y - TITLE) as f64) as f32,
        ))
    }

    /// Runs `window`'s title bar with `events`, and does what it asks.
    fn frame_input(&mut self, window: &Window, events: Vec<egui::Event>) {
        let Some(look) = self.look(window) else {
            return;
        };
        let commands = slot(window)
            .0
            .borrow_mut()
            .run(&look, &self.frames.skin, events);
        self.needs_redraw = true;
        self.frame_commands(window, commands);
    }

    /// What a title bar's buttons and drags asked for, as the app's own
    /// would: a move, minimising, maximising or restoring, closing.
    fn frame_commands(&mut self, window: &Window, commands: Vec<ViewportCommand>) {
        for command in commands {
            match command {
                ViewportCommand::StartDrag => {
                    let Some(pointer) = self.seat.get_pointer() else {
                        continue;
                    };
                    // The button comes up under the grab; `release_grab` tells
                    // the bar on its next run, which this is.
                    if self.frames.pressed.take().is_some() {
                        self.frame_input(window, Vec::new());
                    }
                    let start = GrabStartData {
                        focus: None,
                        button: BTN_LEFT,
                        location: pointer.current_location(),
                    };
                    let serial = SERIAL_COUNTER.next_serial();
                    self.start_move(window.clone(), start, pointer, serial);
                }
                ViewportCommand::Minimized(true) => self.minimize(window),
                ViewportCommand::Maximized(true) => self.maximize(window),
                ViewportCommand::Maximized(false) => self.unmaximize(window, None),
                ViewportCommand::Close => {
                    if let Some(toplevel) = window.toplevel() {
                        toplevel.send_close();
                    } else if let Some(x11) = window.x11_surface() {
                        let _ = x11.close();
                    }
                }
                _ => {}
            }
        }
    }
}

/// Wayland windows get the frame they ask for, and Patina's when they don't
/// mind: Patina apps and GTK draw their own, foot, Qt and SDL's games ask
/// for the server's.
impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.decorate(&toplevel, Mode::ServerSide);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: Mode) {
        self.decorate(&toplevel, mode);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.decorate(&toplevel, Mode::ServerSide);
    }
}

/// `window`'s title, for its title bar.
fn title(window: &Window) -> String {
    match (window.toplevel(), window.x11_surface()) {
        (Some(toplevel), _) => crate::toplevels::describe(toplevel.wl_surface()).0,
        (None, Some(x11)) => x11.title(),
        (None, None) => String::new(),
    }
}

fn click(at: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rectangle<i32, Logical> {
        Rectangle::new((100, 200).into(), (400, 300).into())
    }

    fn at(x: i32, y: i32) -> Option<Part> {
        part_at(rect(), (x as f64 + 0.5, y as f64 + 0.5).into())
    }

    #[test]
    fn the_bar_is_above_the_window_and_the_window_is_its_own() {
        assert_eq!(at(300, 200 - TITLE / 2), Some(Part::Title));
        assert_eq!(at(300, 250), None, "inside the window");
        assert_eq!(at(300, 200 - TITLE - EDGE - 1), None, "past the edge");
        assert_eq!(at(700, 300), None);
    }

    #[test]
    fn edges_and_corners_resize() {
        let edge = |x, y| match at(x, y) {
            Some(Part::Edge(e)) => (e.top, e.bottom, e.left, e.right),
            other => panic!("{other:?} at {x},{y}"),
        };
        assert_eq!(edge(98, 300), (false, false, true, false));
        assert_eq!(edge(502, 300), (false, false, false, true));
        assert_eq!(edge(300, 502), (false, true, false, false));
        assert_eq!(edge(300, 200 - TITLE), (true, false, false, false));
        assert_eq!(edge(98, 498), (false, true, true, false), "a corner");
        assert_eq!(edge(105, 200 - TITLE), (true, false, true, false));
    }

    /// Presses on `frame`'s title bar and drags: whether a move was asked.
    fn drag(frame: &mut Frame, look: &Look, skin: &Skin) -> bool {
        let at = Pos2::new(200.0, 15.0);
        let mut asked = false;
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![click(at, true)],
            vec![egui::Event::PointerMoved(at + egui::vec2(10.0, 0.0))],
            vec![egui::Event::PointerMoved(at + egui::vec2(30.0, 0.0))],
        ] {
            asked |= frame
                .run(look, skin, events)
                .contains(&ViewportCommand::StartDrag);
        }
        asked
    }

    #[test]
    fn the_bar_can_be_dragged_again_after_a_move() {
        let skin = Skin::default_skin();
        let look = Look {
            width: 600,
            scale: 1.0,
            title: "Minecraft Launcher".into(),
            maximized: false,
            skin: skin.name.clone(),
        };
        let mut frame = Frame::default();
        frame.run(&look, &skin, Vec::new());
        assert!(drag(&mut frame, &look, &skin), "the first drag");
        // The move's grab keeps the release; the next run is told of it.
        frame.run(&look, &skin, Vec::new());
        assert!(drag(&mut frame, &look, &skin), "the second");
    }

    /// Paints a frame in every built-in skin, as PNGs in the folder
    /// `PATINA_FRAMES_PREVIEW` names, focused and with the pointer on close:
    /// `PATINA_FRAMES_PREVIEW=dir cargo test -p patina-compositor frames --
    /// --ignored`.
    #[test]
    #[ignore]
    fn preview() {
        let dir = std::path::PathBuf::from(
            std::env::var_os("PATINA_FRAMES_PREVIEW").expect("PATINA_FRAMES_PREVIEW=dir"),
        );
        std::fs::create_dir_all(&dir).unwrap();
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        for skin in skins {
            for (name, hover) in [("", None), ("-close", Some(Pos2::new(580.0, 20.0)))] {
                let look = Look {
                    width: 600,
                    scale: 1.5,
                    title: "Minecraft Launcher".into(),
                    maximized: false,
                    skin: skin.name.clone(),
                };
                let mut frame = Frame::default();
                frame.run(&look, &skin, Vec::new());
                let events = hover.map(egui::Event::PointerMoved).into_iter().collect();
                frame.run(&look, &skin, events);
                let size = buffer_size(&look);
                let rgba: Vec<u8> = frame
                    .pixels
                    .iter()
                    .flat_map(|p| {
                        let [b, g, r, a] = p.to_le_bytes();
                        let un = |c: u8| {
                            if a == 0 {
                                0
                            } else {
                                (c as u32 * 255 / a as u32) as u8
                            }
                        };
                        [un(r), un(g), un(b), a]
                    })
                    .collect();
                let file =
                    std::fs::File::create(dir.join(format!("{}{name}.png", skin.name))).unwrap();
                let mut encoder = png::Encoder::new(file, size.w as u32, size.h as u32);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&rgba)
                    .unwrap();
            }
        }
    }
}
