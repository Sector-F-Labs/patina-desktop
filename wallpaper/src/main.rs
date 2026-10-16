//! patina-wallpaper: the desktop's wallpaper (ADR-0030). An image from the
//! bar's config (`wallpaper = "…"`), scaled to fill each output, on the
//! background layer; without one, the default: the Sector F flask over
//! "Sector F Labs" in Ubuntu Mono, in the skin's colours. The
//! compositor starts it with the bar, and it follows the config file as
//! the desktop's menu changes it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{
    Align2, Color32, ColorImage, FontData, FontFamily, FontId, Rect, TextureHandle, TextureOptions,
    Ui, pos2,
};
use image::DynamicImage;
use image::imageops::FilterType;
use patina::skin::Skin;
use patina_shell::calloop::timer::{TimeoutAction, Timer};
use patina_shell::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell, piece};

const USAGE: &str = "usage: patina-wallpaper

Draws the desktop's wallpaper: patina-bar's `wallpaper` setting, an image
file, scaled to fill each screen, or without one the Sector F Labs logo in
the colours of the bar's skin. Changes to the settings are followed.";

/// The default's lettering, as an egui font family.
const LETTERING: &str = "Ubuntu Mono";

/// How often the bar's config is looked at for changes.
const POLL: Duration = Duration::from_secs(1);

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("-V" | "--version") => {
            println!("patina-wallpaper {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("patina-wallpaper: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options {
        namespace: "patina-wallpaper".into(),
        layer: Layer::Background,
        anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
        size: (0, 0),
        // Under the bar too.
        exclusive_zone: -1,
        margin: (0, 0, 0, 0),
        keyboard: KeyboardInteractivity::None,
        per_output: true,
        toplevels: false,
        click_through: true,
        lock: false,
        window_fallback: false,
    };
    let (host, event_loop) = Host::connect(options, Wallpaper::new())?;
    let config = patina_shell::bar::config_path();
    let mut seen = config.as_deref().and_then(modified);
    event_loop
        .handle()
        .insert_source(Timer::from_duration(POLL), move |_, _, host| {
            let now = config.as_deref().and_then(modified);
            if now != seen {
                seen = now;
                if host.shell.reload() {
                    host.restyle();
                }
            }
            TimeoutAction::ToDuration(POLL)
        })
        .map_err(|e| e.to_string())?;
    host.run(event_loop)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

struct Wallpaper {
    skin: Skin,
    path: Option<PathBuf>,
    image: Option<DynamicImage>,
    /// The image scaled to each output, by output name: its size in pixels
    /// and its texture, in that output's egui context.
    scaled: HashMap<String, ([usize; 2], TextureHandle)>,
    /// The default's logo, the same way: its side in pixels.
    logos: HashMap<String, (u32, TextureHandle)>,
}

impl Wallpaper {
    fn new() -> Self {
        let mut wallpaper = Self {
            skin: Skin::default_skin(),
            path: None,
            image: None,
            scaled: HashMap::new(),
            logos: HashMap::new(),
        };
        wallpaper.reload();
        wallpaper
    }

    /// Reads the wallpaper and skin from the bar's config again; true if
    /// either changed.
    fn reload(&mut self) -> bool {
        let path = patina_shell::bar::wallpaper();
        let skin = piece::skin("patina-wallpaper");
        let changed = path != self.path || skin.name != self.skin.name;
        if path != self.path {
            self.image = path.as_deref().and_then(load);
            self.scaled.clear();
            self.path = path;
        }
        self.skin = skin;
        changed
    }

    /// The image scaled to fill `size` pixels, made once per output and
    /// size.
    fn texture(
        &mut self,
        ctx: &egui::Context,
        output: &str,
        size: [usize; 2],
    ) -> Option<&TextureHandle> {
        let image = self.image.as_ref()?;
        if self.scaled.get(output).is_none_or(|(s, _)| *s != size) {
            let filled = image
                .resize_to_fill(size[0] as u32, size[1] as u32, FilterType::Triangle)
                .to_rgba8();
            let pixels = ColorImage::from_rgba_unmultiplied(
                [filled.width() as usize, filled.height() as usize],
                filled.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("wallpaper-{output}"),
                pixels,
                TextureOptions::LINEAR,
            );
            self.scaled.insert(output.to_string(), (size, texture));
        }
        self.scaled.get(output).map(|(_, t)| t)
    }

    /// The default wallpaper: the flask, and "Sector F Labs" under it, a
    /// little above the middle, on the skin's panel colour, so the dark
    /// default is egui's own dark, and every skin gets its own.
    fn default(&mut self, ui: &Ui, output: &str) {
        let rect = ui.max_rect();
        let v = ui.visuals();
        let painter = ui.painter();
        painter.rect_filled(rect, 0.0, v.panel_fill);

        let side = (rect.width().min(rect.height()) * 0.22).round();
        let letters = side * 0.2;
        let gap = side * 0.12;
        let top = rect.center().y - (side + gap + letters) / 2.0 - rect.height() * 0.04;
        let logo = Rect::from_min_size(
            pos2(rect.center().x - side / 2.0, top),
            egui::vec2(side, side),
        );
        let ink = v.text_color();
        let flask = accent(v.selection.bg_fill, v.panel_fill).unwrap_or(ink.gamma_multiply(0.55));

        let pixels = (side * ui.ctx().pixels_per_point()).round().max(1.0) as u32;
        if self.logos.get(output).is_none_or(|(p, _)| *p != pixels)
            && let Some(image) = patina_system::logo::rasterise(pixels)
        {
            let texture =
                ui.ctx()
                    .load_texture(format!("logo-{output}"), image, TextureOptions::LINEAR);
            self.logos.insert(output.to_string(), (pixels, texture));
        }
        if let Some((_, texture)) = self.logos.get(output) {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(texture.id(), logo, uv, flask);
        }
        painter.text(
            pos2(rect.center().x, logo.bottom() + gap),
            Align2::CENTER_TOP,
            "Sector F Labs",
            FontId::new(letters, FontFamily::Name(LETTERING.into())),
            ink.gamma_multiply(0.45),
        );
    }
}

/// The skin's accent for the flask, if it stands out from `background`
/// enough to see; some skins' selection colour is close to their panels.
fn accent(colour: Color32, background: Color32) -> Option<Color32> {
    let luma = |c: Color32| 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    ((luma(colour) - luma(background)).abs() > 40.0).then_some(colour)
}

/// Decodes the image at `path`, saying why not if it can't.
fn load(path: &Path) -> Option<DynamicImage> {
    image::open(path)
        .map_err(|e| eprintln!("patina-wallpaper: {}: {e}", path.display()))
        .ok()
}

impl Shell for Wallpaper {
    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        patina::appearance::watch(ctx);
        ctx.add_font(FontInsert::new(
            LETTERING,
            FontData::from_static(patina_system::logo::UBUNTU_MONO),
            vec![InsertFontFamily {
                family: FontFamily::Name(LETTERING.into()),
                priority: FontPriority::Highest,
            }],
        ));
    }

    fn restyle(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, output: &str) {
        let rect = ui.max_rect();
        let ppp = ui.ctx().pixels_per_point();
        let size = [
            (rect.width() * ppp).round().max(1.0) as usize,
            (rect.height() * ppp).round().max(1.0) as usize,
        ];
        let ctx = ui.ctx().clone();
        match self.texture(&ctx, output, size) {
            Some(texture) => {
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(texture.id(), rect, uv, Color32::WHITE);
            }
            None => self.default(ui, output),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws the default in every built-in skin, as PNGs in the folder
    /// PATINA_WALLPAPER_PREVIEW names, to look at.
    #[test]
    #[ignore]
    fn preview_the_default() {
        let dir = PathBuf::from(std::env::var("PATINA_WALLPAPER_PREVIEW").unwrap());
        let (skins, _) = patina::skin::load_all(patina::skin::BUILT_IN, &[]);
        let (w, h) = (1600usize, 1000usize);
        for skin in skins {
            let mut wallpaper = Wallpaper {
                skin: skin.clone(),
                path: None,
                image: None,
                scaled: HashMap::new(),
                logos: HashMap::new(),
            };
            let ctx = egui::Context::default();
            wallpaper.setup(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            // Twice: fonts load on the first pass.
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        egui::vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| wallpaper.ui(ui, "test"));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            let file = dir.join(format!("{}.png", skin.name));
            image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
        }
    }
}
