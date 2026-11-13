//! Screenshots, with the Mac's keys (ADR-0034): Super+Shift+3 takes the
//! whole desktop, Super+Shift+4 the focused window. The Print key (and
//! Super+Shift+5) offers the choice, a selection too (ADR-0091, `menu.rs`,
//! `select.rs`). The compositor draws what it would show into an image of
//! its own, with whichever renderer it's using, and a thread saves it as a
//! PNG in ~/Pictures/Screenshots, named as macOS names them.

pub mod menu;
pub mod select;

use crate::layout::View;
use std::path::{Path, PathBuf};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
            damage::OutputDamageTracker,
            element::{
                AsRenderElements, Kind, RenderElement,
                solid::{SolidColorBuffer, SolidColorRenderElement},
                surface::WaylandSurfaceRenderElement,
            },
        },
    },
    desktop::{Space, Window},
    output::Output,
    render_elements,
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
};
use std::time::{Duration, Instant};

use crate::state::State;

/// What a screenshot takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shot {
    /// Everything on screen but the pointer (Super+Shift+3).
    Desktop,
    /// The focused window, to its edges, without the desktop behind it
    /// (Super+Shift+4).
    Window,
}

/// The Print key's choices and a selection being made (ADR-0091).
#[derive(Default)]
pub struct Shots {
    /// patina-screenshot is showing the choices.
    pub menu_open: bool,
    /// The window focused when they were opened, to take if it's chosen.
    pub window: Option<Window>,
    /// Where patina-screenshot's choices and Stop reach the event loop,
    /// once it's been set up.
    pub events: Option<smithay::reexports::calloop::channel::Sender<menu::Event>>,
    pub selecting: Option<select::Selecting>,
}

/// Pixels as a PNG wants them: rows from the top, RGBA, not premultiplied.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl State {
    /// Takes `shot` and saves it on a thread. Nothing happens without a
    /// focused window to take.
    pub fn screenshot(&mut self, shot: Shot) {
        let window = match shot {
            Shot::Desktop => None,
            Shot::Window => match self.focused_window() {
                Some(window) => Some(window),
                None => {
                    eprintln!("patina-compositor: no window to take a screenshot of");
                    return;
                }
            },
        };
        self.take_screenshot(window, None);
    }

    /// Takes `window`, or else the desktop, or `area` of it, and saves it
    /// on a thread.
    pub fn take_screenshot(
        &mut self,
        window: Option<Window>,
        area: Option<Rectangle<i32, Logical>>,
    ) {
        let framed = window.as_ref().is_some_and(|w| self.framed(w));
        let (space, output) = (&self.space, &self.output);
        let fullscreen = self.fullscreen.as_ref();
        let drawn = self.fullscreen_drawn();
        let drawn = drawn.as_ref();
        let window = window.as_ref();
        let picture = if let Some(u) = self.udev.as_mut() {
            take::<_, smithay::backend::renderer::gles::GlesTexture>(
                &mut u.renderer,
                space,
                output,
                window,
                fullscreen,
                drawn,
                framed,
            )
        } else if let Some(w) = self.winit.as_mut() {
            take::<_, smithay::backend::renderer::gles::GlesTexture>(
                w.backend.renderer(),
                space,
                output,
                window,
                fullscreen,
                drawn,
                framed,
            )
        } else if let Some(n) = self.nested.as_mut() {
            take::<_, smithay::reexports::pixman::Image<'static, 'static>>(
                &mut n.renderer,
                space,
                output,
                window,
                fullscreen,
                drawn,
                framed,
            )
        } else {
            return;
        };
        let screen = self.space.output_geometry(&self.output);
        let picture = match (picture, area, screen) {
            (Ok(picture), Some(area), Some(screen)) => {
                let scale = self.output.current_scale().fractional_scale();
                crop(
                    &picture,
                    Rectangle::new(
                        (area.loc - screen.loc).to_physical_precise_round(scale),
                        area.size.to_physical_precise_round(scale),
                    ),
                )
            }
            (picture, ..) => picture,
        };
        match picture {
            Ok(picture) => {
                let area = match (window, area) {
                    (Some(window), _) => self.outer_rect(window),
                    (None, Some(area)) => area.intersection(screen.unwrap_or(area)),
                    (None, None) => screen,
                };
                if let Some(area) = area {
                    self.flash = Some(Flash::new(area));
                    self.needs_redraw = true;
                }
                // On the clipboard too, whether or not a history is kept (ADR-0116).
                let pictures = self
                    .config
                    .clipboard
                    .screenshots
                    .then(|| self.clipboard_pictures())
                    .flatten();
                let _ = std::thread::Builder::new()
                    .name("screenshot".into())
                    .spawn(move || {
                        let png = match encode_png(&picture) {
                            Ok(png) => png,
                            Err(e) => return eprintln!("patina-compositor: screenshot: {e}"),
                        };
                        match save(&png) {
                            Ok(path) => eprintln!("patina-compositor: saved {}", path.display()),
                            Err(e) => eprintln!("patina-compositor: screenshot: {e}"),
                        }
                        if let Some(pictures) = pictures {
                            pictures.send(png);
                        }
                    });
            }
            Err(e) => eprintln!("patina-compositor: screenshot: {e}"),
        }
    }
}

/// How long the flash takes to fade, and how white it starts.
const FLASH: Duration = Duration::from_millis(350);
const FLASH_START: f32 = 0.7;

/// A white flash over what a screenshot took, fading out, so it's seen to
/// have happened. It's drawn after the picture is taken, so it isn't in it.
pub struct Flash {
    started: Instant,
    area: Rectangle<i32, Logical>,
    buffer: SolidColorBuffer,
}

impl Flash {
    fn new(area: Rectangle<i32, Logical>) -> Self {
        Self {
            started: Instant::now(),
            area,
            buffer: SolidColorBuffer::new(area.size, [0.0; 4]),
        }
    }

    /// How white the flash is `t` into it: fast at first, easing out.
    fn opacity(t: Duration) -> f32 {
        let left = 1.0 - (t.as_secs_f32() / FLASH.as_secs_f32()).min(1.0);
        FLASH_START * left * left
    }
}

render_elements! {
    /// What a backend drawing into a window draws: the locker or a flash
    /// over the desktop.
    pub Overlay<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Flash=SolidColorRenderElement,
    Desktop=crate::frames::DesktopElement<R>,
}

/// What's being dragged, on top, then the flash and tiling preview, then
/// the locker, then `desktop` (the space, unless locked) on `output`, for a
/// backend drawing into a window.
#[allow(clippy::too_many_arguments)]
pub fn overlay<R>(
    renderer: &mut R,
    lock_surface: Option<&smithay::reexports::wayland_server::protocol::wl_surface::WlSurface>,
    scale: f64,
    flash: Vec<SolidColorRenderElement>,
    dnd_icon: Option<&(
        smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        smithay::utils::Point<f64, smithay::utils::Logical>,
    )>,
    fullscreen: Option<&(
        smithay::desktop::Window,
        smithay::utils::Point<i32, smithay::utils::Logical>,
    )>,
    desktop: Option<&Space<Window>>,
    output: &Output,
    fullscreen_window: Option<&Window>,
) -> Vec<Overlay<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    crate::dnd::elements(renderer, dnd_icon, scale)
        .into_iter()
        .map(Overlay::Surface)
        .chain(flash.into_iter().map(Overlay::Flash))
        .chain(
            crate::fullscreen::elements(renderer, fullscreen, scale)
                .into_iter()
                .map(Overlay::Surface),
        )
        .chain(
            crate::lock::elements(renderer, lock_surface, scale)
                .into_iter()
                .map(Overlay::Surface),
        )
        .chain(
            desktop
                .map(|space| {
                    crate::frames::desktop_elements(
                        renderer,
                        space,
                        output,
                        fullscreen_window,
                        false,
                    )
                })
                .unwrap_or_default()
                .into_iter()
                .map(Overlay::Desktop),
        )
        .collect()
}

impl State {
    /// The flash for this frame, if one is fading; asks for the next frame
    /// while it lasts. Call once a frame, after `needs_redraw` is cleared.
    pub fn flash_element(&mut self, view: View) -> Option<SolidColorRenderElement> {
        let flash = self.flash.as_mut()?;
        let t = flash.started.elapsed();
        if t >= FLASH {
            self.flash = None;
            return None;
        }
        let a = Flash::opacity(t);
        // Premultiplied white.
        flash.buffer.update(flash.area.size, [a; 4]);
        let scale = view.scale;
        self.needs_redraw = true;
        Some(SolidColorRenderElement::from_buffer(
            &flash.buffer,
            (flash.area.loc - view.area.loc).to_physical_precise_round(scale),
            scale,
            1.0,
            Kind::Unspecified,
        ))
    }
}

/// The desktop, or `window` alone, drawn by `renderer` into a `T`.
/// `fullscreen` is drawn without its frame, and `window` with its own if
/// it's `framed` (ADR-0077). `drawn` is the fullscreen window on top, which
/// the screen draws over the bar, so the picture does too (ADR-0102).
fn take<R, T>(
    renderer: &mut R,
    space: &Space<Window>,
    output: &Output,
    window: Option<&Window>,
    fullscreen: Option<&Window>,
    drawn: Option<&(Window, Point<i32, Logical>)>,
    framed: bool,
) -> Result<Picture, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let scale = output.current_scale().fractional_scale();
    match window {
        None => {
            let size = output.current_mode().ok_or("the output has no mode")?.size;
            let mut elements: Vec<crate::frames::DesktopElement<R>> =
                crate::fullscreen::elements(renderer, drawn, scale)
                    .into_iter()
                    .map(crate::frames::DesktopElement::Surface)
                    .collect();
            elements.extend(crate::frames::desktop_elements(
                renderer, space, output, fullscreen, true,
            ));
            draw::<R, T, _>(renderer, &elements, size, scale, crate::nested::BACKGROUND)
        }
        Some(window) => {
            // The window's own edges, without the shadow a client may draw
            // around them.
            // around them; with the frame drawn for it, if it has one.
            let geometry = window.geometry();
            let top = if framed { crate::frames::TITLE } else { 0 };
            let size = Size::<i32, Logical>::from((geometry.size.w, geometry.size.h + top))
                .to_physical_precise_round(scale);
            let at = (Point::<i32, Logical>::from((0, top)) - geometry.loc)
                .to_physical_precise_round(scale);
            let mut elements: Vec<crate::frames::DesktopElement<R>> = window
                .render_elements::<WaylandSurfaceRenderElement<R>>(renderer, at, scale.into(), 1.0)
                .into_iter()
                .map(crate::frames::DesktopElement::Surface)
                .collect();
            if framed {
                elements.extend(
                    crate::frames::screenshot_element(renderer, window, scale)
                        .map(crate::frames::DesktopElement::Frame),
                );
            }
            draw::<R, T, _>(renderer, &elements, size, scale, [0.0; 4])
        }
    }
}

/// Draws `elements` over `clear` into a new `size` image and reads it back.
fn draw<R, T, E>(
    renderer: &mut R,
    elements: &[E],
    size: Size<i32, Physical>,
    scale: f64,
    clear: [f32; 4],
) -> Result<Picture, String>
where
    R: Renderer + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Texture,
    E: RenderElement<R>,
{
    let mut rgba = read_back::<R, T, E>(renderer, elements, size, scale, clear)?;
    unpremultiply(&mut rgba);
    Ok(Picture {
        width: size.w as u32,
        height: size.h as u32,
        rgba,
    })
}

/// Draws `elements` over `clear` into a new `size` image and reads it
/// back: rows from the top, RGBA, premultiplied as renderers blend. Also
/// what screen capture copies into its clients' buffers (ADR-0092).
pub fn read_back<R, T, E>(
    renderer: &mut R,
    elements: &[E],
    size: Size<i32, Physical>,
    scale: f64,
    clear: [f32; 4],
) -> Result<Vec<u8>, String>
where
    R: Renderer + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Texture,
    E: RenderElement<R>,
{
    if size.w <= 0 || size.h <= 0 {
        return Err(format!("nothing to take at {}x{}", size.w, size.h));
    }
    let buffer = size.to_logical(1).to_buffer(1, Transform::Normal);
    // Abgr8888 is R, G, B, A in memory, as a PNG wants.
    let mut target = renderer
        .create_buffer(Fourcc::Abgr8888, buffer)
        .map_err(|e| format!("no {}x{} image: {e:?}", size.w, size.h))?;
    let mapping = {
        let mut framebuffer = renderer.bind(&mut target).map_err(|e| format!("{e:?}"))?;
        OutputDamageTracker::new(size, scale, Transform::Normal)
            .render_output(renderer, &mut framebuffer, 0, elements, clear)
            .map_err(|e| format!("drawing: {e:?}"))?;
        renderer
            .copy_framebuffer(&framebuffer, Rectangle::from_size(buffer), Fourcc::Abgr8888)
            .map_err(|e| format!("reading back: {e:?}"))?
    };
    Ok(renderer
        .map_texture(&mapping)
        .map_err(|e| format!("reading back: {e:?}"))?
        .to_vec())
}

/// `area` of `picture`, as much of it as is in the picture.
fn crop(picture: &Picture, area: Rectangle<i32, Physical>) -> Result<Picture, String> {
    let whole = Rectangle::from_size((picture.width as i32, picture.height as i32).into());
    let area = area
        .intersection(whole)
        .filter(|a| a.size.w > 0 && a.size.h > 0)
        .ok_or("the selection is off the screen")?;
    let stride = picture.width as usize * 4;
    let (x, w) = (area.loc.x as usize * 4, area.size.w as usize * 4);
    let rgba = (area.loc.y..area.loc.y + area.size.h)
        .flat_map(|y| {
            let row = y as usize * stride + x;
            &picture.rgba[row..row + w]
        })
        .copied()
        .collect();
    Ok(Picture {
        width: area.size.w as u32,
        height: area.size.h as u32,
        rgba,
    })
}

/// Renderers blend premultiplied colour; a PNG's isn't.
fn unpremultiply(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let a = pixel[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut pixel[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// Writes the PNG `png` into the screenshots folder; returns where.
fn save(png: &[u8]) -> Result<PathBuf, String> {
    let folder = folder().ok_or("no home folder")?;
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = free_name(&folder, &name(chrono::Local::now()));
    std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// ~/Pictures/Screenshots, or wherever the user's Pictures folder is.
fn folder() -> Option<PathBuf> {
    let pictures = dirs::picture_dir().or_else(|| Some(dirs::home_dir()?.join("Pictures")))?;
    Some(pictures.join("Screenshots"))
}

/// "Screenshot 2026-10-15 at 21.04.33", as macOS names them.
fn name(time: chrono::DateTime<chrono::Local>) -> String {
    time.format("Screenshot %Y-%m-%d at %H.%M.%S").to_string()
}

/// `name`.png in `folder`, or "`name` (2).png" and on if it's taken (two
/// in one second).
fn free_name(folder: &Path, name: &str) -> PathBuf {
    let mut path = folder.join(format!("{name}.png"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{name} ({n}).png"));
        n += 1;
    }
    path
}

/// `picture` as PNG bytes.
fn encode_png(picture: &Picture) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, picture.width, picture.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer
        .write_image_data(&picture.rgba)
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn named_as_macos_names_them() {
        let time = chrono::Local
            .with_ymd_and_hms(2026, 10, 15, 9, 4, 33)
            .unwrap();
        assert_eq!(name(time), "Screenshot 2026-10-15 at 09.04.33");
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let folder = std::env::temp_dir().join(format!("patina-shots-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert_eq!(free_name(&folder, "a"), folder.join("a.png"));
        std::fs::write(folder.join("a.png"), b"").unwrap();
        assert_eq!(free_name(&folder, "a"), folder.join("a (2).png"));
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn the_flash_fades_out() {
        assert_eq!(Flash::opacity(Duration::ZERO), FLASH_START);
        let half = Flash::opacity(FLASH / 2);
        assert!(half > 0.0 && half < FLASH_START / 2.0, "eases out");
        assert_eq!(Flash::opacity(FLASH), 0.0);
    }

    #[test]
    fn a_selection_is_cut_out_and_kept_on_the_picture() {
        // 3x2, each pixel's red its index.
        let picture = Picture {
            width: 3,
            height: 2,
            rgba: (0..6).flat_map(|i| [i, 0, 0, 255]).collect(),
        };
        let cut = crop(&picture, Rectangle::new((1, 0).into(), (2, 2).into())).unwrap();
        assert_eq!((cut.width, cut.height), (2, 2));
        let reds: Vec<u8> = cut.rgba.chunks(4).map(|p| p[0]).collect();
        assert_eq!(reds, [1, 2, 4, 5]);
        let past = crop(&picture, Rectangle::new((2, 1).into(), (5, 5).into())).unwrap();
        assert_eq!((past.width, past.height), (1, 1));
        assert!(crop(&picture, Rectangle::new((5, 5).into(), (2, 2).into())).is_err());
    }

    #[test]
    fn colour_is_unpremultiplied() {
        let mut rgba = [64, 32, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        unpremultiply(&mut rgba);
        assert_eq!(rgba, [128, 64, 0, 128, 10, 20, 30, 255, 0, 0, 0, 0]);
    }

    /// Red over the top half of a 4x2 picture, on blue: it should come
    /// back the right way up, at its size.
    fn upright<R, T>(renderer: &mut R)
    where
        R: Renderer + Offscreen<T> + Bind<T> + ExportMem,
        R::TextureId: Texture,
    {
        use smithay::backend::renderer::element::solid::SolidColorRenderElement;
        use smithay::backend::renderer::element::{Id, Kind};
        use smithay::backend::renderer::utils::CommitCounter;
        let red = SolidColorRenderElement::new(
            Id::new(),
            Rectangle::new((0, 0).into(), (4, 1).into()),
            CommitCounter::default(),
            [1.0, 0.0, 0.0, 1.0],
            Kind::Unspecified,
        );
        let picture =
            draw::<R, T, _>(renderer, &[red], (4, 2).into(), 1.0, [0.0, 0.0, 1.0, 1.0]).unwrap();
        assert_eq!((picture.width, picture.height), (4, 2));
        assert_eq!(&picture.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&picture.rgba[4 * 4..4 * 4 + 4], &[0, 0, 255, 255]);
    }

    /// Pixman, as the nested backend draws.
    #[test]
    fn pixman_draws_upright() {
        let mut renderer = smithay::backend::renderer::pixman::PixmanRenderer::new().unwrap();
        upright::<_, smithay::reexports::pixman::Image<'static, 'static>>(&mut renderer);
    }

    /// OpenGL ES, as on the hardware and in a window; needs a GPU.
    #[test]
    #[ignore]
    fn gles_draws_upright() {
        use smithay::backend::egl::{EGLContext, EGLDevice, EGLDisplay};
        let device = EGLDevice::enumerate()
            .unwrap()
            .find(|d| d.try_get_render_node().ok().flatten().is_some())
            .expect("a GPU");
        // SAFETY: the display outlives the renderer, both in this test.
        let display = unsafe { EGLDisplay::new(device) }.unwrap();
        let context = EGLContext::new(&display).unwrap();
        let mut renderer =
            unsafe { smithay::backend::renderer::gles::GlesRenderer::new(context) }.unwrap();
        upright::<_, smithay::backend::renderer::gles::GlesTexture>(&mut renderer);
    }
}
