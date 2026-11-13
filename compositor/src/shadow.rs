//! Shadows for windows that ask for one, through KDE's `org_kde_kwin_shadow`
//! protocol. A client that draws its own frame, as every Patina
//! app does, gives the compositor eight tiles around the window (the four
//! corners and the four sides) and how far they reach outside it; the
//! compositor draws them under the window. Nothing is added to the window's
//! own surface, so its geometry stays what it is and tiling and snapping see
//! the window, not its shadow. KWin has the same protocol, so the apps get
//! shadows there too.
//!
//! A shadow takes effect when the client commits it, not with the surface's
//! next commit as KWin has it. The tiles are copied out of their shm buffers
//! then, which are released at once.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportMem, Renderer, Texture,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
        },
    },
    desktop::Window,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New,
            protocol::{wl_buffer::WlBuffer, wl_shm, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, Size, Transform},
    wayland::{
        compositor::with_states,
        seat::WaylandFocus,
        shm::{BufferData, with_buffer_contents},
    },
};
use wayland_protocols_plasma::shadow::server::{
    org_kde_kwin_shadow::{self, OrgKdeKwinShadow},
    org_kde_kwin_shadow_manager::{self, OrgKdeKwinShadowManager},
};

use crate::state::State;

/// The tiles, in the order the protocol's `attach_*` requests come in.
const LEFT: usize = 0;
const TOP_LEFT: usize = 1;
const TOP: usize = 2;
const TOP_RIGHT: usize = 3;
const RIGHT: usize = 4;
const BOTTOM_RIGHT: usize = 5;
const BOTTOM: usize = 6;
const BOTTOM_LEFT: usize = 7;

/// Offsets, in the order of `set_left_offset`, `set_top_offset`,
/// `set_right_offset` and `set_bottom_offset`.
const OFFSET_LEFT: usize = 0;
const OFFSET_TOP: usize = 1;
const OFFSET_RIGHT: usize = 2;
const OFFSET_BOTTOM: usize = 3;

/// The most a tile may be on a side, a defence against a client asking for
/// something that fills the screen.
const MAX_TILE: i32 = 512;

/// One of the eight pieces, copied out of the client's buffer.
#[derive(Clone)]
struct Tile {
    buffer: MemoryRenderBuffer,
    size: Size<i32, Logical>,
}

/// A surface's shadow, as the client last committed it.
#[derive(Clone, Default)]
pub struct Shadow {
    tiles: [Option<Tile>; 8],
    /// How far the shadow reaches outside the window's geometry: left, top,
    /// right, bottom, in logical pixels.
    offsets: [f64; 4],
}

/// Per surface, in its `data_map`.
type Slot = Mutex<Option<Shadow>>;

/// What a client has attached to its `org_kde_kwin_shadow` so far.
#[derive(Default)]
struct Pending {
    buffers: [Option<WlBuffer>; 8],
    offsets: [f64; 4],
}

/// The user data of an `org_kde_kwin_shadow`.
pub struct ShadowObject {
    surface: WlSurface,
    pending: Mutex<Pending>,
}

/// Advertises the protocol.
pub fn init(display: &DisplayHandle) {
    display.create_global::<State, OrgKdeKwinShadowManager, ()>(2, ());
}

fn set(surface: &WlSurface, shadow: Option<Shadow>) {
    with_states(surface, |states| {
        let slot = states.data_map.get_or_insert_threadsafe(Slot::default);
        *slot.lock().unwrap() = shadow;
    });
}

impl GlobalDispatch<OrgKdeKwinShadowManager, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<OrgKdeKwinShadowManager>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<OrgKdeKwinShadowManager, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &OrgKdeKwinShadowManager,
        request: org_kde_kwin_shadow_manager::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            org_kde_kwin_shadow_manager::Request::Create { id, surface } => {
                data_init.init(
                    id,
                    ShadowObject {
                        surface,
                        pending: Mutex::default(),
                    },
                );
            }
            org_kde_kwin_shadow_manager::Request::Unset { surface } => set(&surface, None),
            _ => {}
        }
    }
}

impl Dispatch<OrgKdeKwinShadow, ShadowObject> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &OrgKdeKwinShadow,
        request: org_kde_kwin_shadow::Request,
        object: &ShadowObject,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        use org_kde_kwin_shadow::Request as R;
        let mut pending = object.pending.lock().unwrap();
        let mut attach = |tile: usize, buffer: WlBuffer| pending.buffers[tile] = Some(buffer);
        match request {
            R::AttachLeft { buffer } => attach(LEFT, buffer),
            R::AttachTopLeft { buffer } => attach(TOP_LEFT, buffer),
            R::AttachTop { buffer } => attach(TOP, buffer),
            R::AttachTopRight { buffer } => attach(TOP_RIGHT, buffer),
            R::AttachRight { buffer } => attach(RIGHT, buffer),
            R::AttachBottomRight { buffer } => attach(BOTTOM_RIGHT, buffer),
            R::AttachBottom { buffer } => attach(BOTTOM, buffer),
            R::AttachBottomLeft { buffer } => attach(BOTTOM_LEFT, buffer),
            R::SetLeftOffset { offset } => pending.offsets[OFFSET_LEFT] = offset,
            R::SetTopOffset { offset } => pending.offsets[OFFSET_TOP] = offset,
            R::SetRightOffset { offset } => pending.offsets[OFFSET_RIGHT] = offset,
            R::SetBottomOffset { offset } => pending.offsets[OFFSET_BOTTOM] = offset,
            R::Commit => {
                let mut shadow = Shadow {
                    offsets: pending.offsets,
                    ..Shadow::default()
                };
                for (tile, buffer) in pending.buffers.iter_mut().enumerate() {
                    if let Some(buffer) = buffer.take() {
                        shadow.tiles[tile] = read_tile(&buffer);
                        buffer.release();
                    }
                }
                set(&object.surface, Some(shadow));
            }
            // Destroying it removes the shadow, as the protocol says.
            R::Destroy => set(&object.surface, None),
            _ => {}
        }
    }
}

/// `buffer`'s pixels, if it's an shm buffer of a size and format we can use.
fn read_tile(buffer: &WlBuffer) -> Option<Tile> {
    with_buffer_contents(buffer, |ptr, len, data: BufferData| {
        let (w, h) = (data.width, data.height);
        if w < 1 || h < 1 || w > MAX_TILE || h > MAX_TILE {
            return None;
        }
        if !matches!(data.format, wl_shm::Format::Argb8888) {
            return None;
        }
        let (stride, offset) = (data.stride as usize, data.offset as usize);
        let row = w as usize * 4;
        if data.stride < 0 || data.offset < 0 || offset + stride * (h as usize - 1) + row > len {
            return None;
        }
        // SAFETY: the pool is mapped for `len` bytes, and the rows are inside
        // it, checked above.
        let memory = unsafe { std::slice::from_raw_parts(ptr, len) };
        let mut bytes = Vec::with_capacity(row * h as usize);
        for y in 0..h as usize {
            bytes.extend_from_slice(&memory[offset + y * stride..offset + y * stride + row]);
        }
        Some(Tile {
            buffer: MemoryRenderBuffer::from_slice(
                &bytes,
                Fourcc::Argb8888,
                (w, h),
                1,
                Transform::Normal,
                None,
            ),
            size: (w, h).into(),
        })
    })
    .ok()
    .flatten()
}

/// Whether shadows are drawn at all (`[shadows] enabled`, which Settings'
/// Appearance page changes). The renderer reads it where it has no `State`.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Turns the shadows on or off, from the config.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Windows that can't ask for a shadow themselves, so they get the default one
/// here when they haven't asked (ADR-0138): a window that's a viewport of
/// another's, as Files' Quick Look is, is made by the toolkit, which gives out
/// no handle to its surface for the app to ask with.
const UNABLE_TO_ASK: &[&str] = &["com.sectorflabs.Files.QuickLook"];

/// The shadow patina-ui gives a window that asks for the default one: the
/// same eight tiles, made here.
fn default_shadow() -> &'static Shadow {
    static DEFAULT: std::sync::OnceLock<Shadow> = std::sync::OnceLock::new();
    DEFAULT.get_or_init(|| {
        let (tiles, offsets) = patina::frame::shadow::tiles(Default::default());
        let mut shadow = Shadow {
            offsets: offsets.map(f64::from),
            ..Shadow::default()
        };
        for (i, (w, h, bytes)) in tiles.into_iter().enumerate() {
            shadow.tiles[i] = Some(Tile {
                buffer: MemoryRenderBuffer::from_slice(
                    &bytes,
                    Fourcc::Argb8888,
                    (w, h),
                    1,
                    Transform::Normal,
                    None,
                ),
                size: (w, h).into(),
            });
        }
        shadow
    })
}

/// How much of its shadow a window keeps when it isn't the focused one.
/// Firefox's, on a white background, is 27 to 34 of 255 dark beside the
/// window unfocused and 61 to 79 focused: about 0.45.
const UNFOCUSED: f32 = 0.45;

/// `window`'s shadow tiles, if it has asked for one and isn't maximised
/// (the window's own geometry then fills what it was given), around its
/// geometry at `location` (relative to the output) and under it. The shadow
/// is lighter while the window isn't focused (`UNFOCUSED`).
pub fn elements<R>(
    renderer: &mut R,
    window: &Window,
    location: Point<i32, Logical>,
    scale: f64,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    if !ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    if let Some(toplevel) = window.toplevel()
        && toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Maximized)
    {
        return Vec::new();
    }
    let focused = window.toplevel().is_none_or(|toplevel| {
        toplevel
            .current_state()
            .states
            .contains(xdg_toplevel::State::Activated)
    });
    let alpha = if focused { 1.0 } else { UNFOCUSED };
    let Some(surface) = window.wl_surface() else {
        return Vec::new();
    };
    let own = window.toplevel().is_some_and(|t| {
        let app_id = crate::toplevels::describe(t.wl_surface()).1;
        UNABLE_TO_ASK.contains(&app_id.as_str())
    });
    around(
        renderer,
        &surface,
        Rectangle::new(location, window.geometry().size),
        scale,
        alpha,
        own,
    )
}

/// A layer surface's shadow tiles, if it has asked for one (the HUD's pieces
/// do, ADR-0113), around `geometry`, its place on the output, and under it.
pub fn layer_elements<R>(
    renderer: &mut R,
    surface: &WlSurface,
    geometry: Rectangle<i32, Logical>,
    scale: f64,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    if !ENABLED.load(Ordering::Relaxed) {
        return Vec::new();
    }
    around(renderer, surface, geometry, scale, 1.0, false)
}

/// `surface`'s shadow tiles around `window`, the rectangle that is the
/// window (or the layer surface) on the output, drawn at `alpha`.
fn around<R>(
    renderer: &mut R,
    surface: &WlSurface,
    window: Rectangle<i32, Logical>,
    scale: f64,
    alpha: f32,
    default: bool,
) -> Vec<MemoryRenderBufferRenderElement<R>>
where
    R: Renderer + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let (location, size) = (window.loc, window.size);
    let asked = with_states(surface, |states| {
        states
            .data_map
            .get::<Slot>()
            .and_then(|slot| slot.lock().unwrap().clone())
    });
    // With `default`, one that hasn't asked has the default shadow.
    let Some(shadow) = asked.or_else(|| default.then(|| default_shadow().clone())) else {
        return Vec::new();
    };
    let [left, top, right, bottom] = shadow.offsets.map(|o| o.round() as i32);
    // The rectangle the shadow fills, around the window.
    let outer = Rectangle::<i32, Logical>::new(
        (location.x - left, location.y - top).into(),
        (size.w + left + right, size.h + top + bottom).into(),
    );
    let tile = |i: usize| shadow.tiles[i].as_ref();
    let width = |i: usize| tile(i).map_or(0, |t| t.size.w);
    let height = |i: usize| tile(i).map_or(0, |t| t.size.h);
    // Each tile's place, as KWin lays them out: the corners at the outer
    // rectangle's corners, the sides stretched between them.
    let (x0, y0) = (outer.loc.x, outer.loc.y);
    let (x1, y1) = (x0 + outer.size.w, y0 + outer.size.h);
    let places: [(usize, Rectangle<i32, Logical>); 8] = [
        (
            TOP_LEFT,
            Rectangle::new((x0, y0).into(), (width(TOP_LEFT), height(TOP_LEFT)).into()),
        ),
        (
            TOP,
            Rectangle::new(
                (x0 + width(TOP_LEFT), y0).into(),
                (
                    (outer.size.w - width(TOP_LEFT) - width(TOP_RIGHT)).max(0),
                    height(TOP),
                )
                    .into(),
            ),
        ),
        (
            TOP_RIGHT,
            Rectangle::new(
                (x1 - width(TOP_RIGHT), y0).into(),
                (width(TOP_RIGHT), height(TOP_RIGHT)).into(),
            ),
        ),
        (
            RIGHT,
            Rectangle::new(
                (x1 - width(RIGHT), y0 + height(TOP_RIGHT)).into(),
                (
                    width(RIGHT),
                    (outer.size.h - height(TOP_RIGHT) - height(BOTTOM_RIGHT)).max(0),
                )
                    .into(),
            ),
        ),
        (
            BOTTOM_RIGHT,
            Rectangle::new(
                (x1 - width(BOTTOM_RIGHT), y1 - height(BOTTOM_RIGHT)).into(),
                (width(BOTTOM_RIGHT), height(BOTTOM_RIGHT)).into(),
            ),
        ),
        (
            BOTTOM,
            Rectangle::new(
                (x0 + width(BOTTOM_LEFT), y1 - height(BOTTOM)).into(),
                (
                    (outer.size.w - width(BOTTOM_LEFT) - width(BOTTOM_RIGHT)).max(0),
                    height(BOTTOM),
                )
                    .into(),
            ),
        ),
        (
            BOTTOM_LEFT,
            Rectangle::new(
                (x0, y1 - height(BOTTOM_LEFT)).into(),
                (width(BOTTOM_LEFT), height(BOTTOM_LEFT)).into(),
            ),
        ),
        (
            LEFT,
            Rectangle::new(
                (x0, y0 + height(TOP_LEFT)).into(),
                (
                    width(LEFT),
                    (outer.size.h - height(TOP_LEFT) - height(BOTTOM_LEFT)).max(0),
                )
                    .into(),
            ),
        ),
    ];
    places
        .into_iter()
        .filter_map(|(i, place)| {
            let tile = tile(i)?;
            if place.size.w < 1 || place.size.h < 1 {
                return None;
            }
            MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                place
                    .loc
                    .to_f64()
                    .to_physical(scale)
                    .to_i32_round::<i32>()
                    .to_f64(),
                &tile.buffer,
                Some(alpha),
                Some(Rectangle::from_size(
                    (tile.size.w as f64, tile.size.h as f64).into(),
                )),
                Some(place.size),
                Kind::Unspecified,
            )
            .ok()
        })
        .collect()
}
