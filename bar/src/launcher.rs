//! The launcher button: the Sector F flask, which runs a command (wofi by
//! default) when clicked. The logo is rasterised at the exact pixel size it
//! is drawn at, as a white mask, and tinted from the skin's text colour, so
//! skins reach it like everything else the bar paints.

use std::process::{Command, Stdio};

use egui::{ColorImage, Id, Sense, TextureHandle, TextureOptions, Ui, vec2};

const LOGO: &str = include_str!("../assets/sector-f.svg");

/// Space around the logo inside the button, in points.
const PADDING: f32 = 4.0;

/// Draws the button; runs `command` when it's clicked.
pub fn button(ui: &mut Ui, command: &str) {
    let side = ui.available_height();
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    let v = ui.visuals();
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, v.widgets.hovered.weak_bg_fill);
    }
    let tint = if response.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    let icon = rect.shrink(PADDING);
    let pixels = (icon.width() * ui.ctx().pixels_per_point())
        .round()
        .max(1.0) as u32;
    if let Some(texture) = texture(ui.ctx(), pixels) {
        ui.painter().image(
            texture.id(),
            icon,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    }
    if response.clicked() {
        run(command);
    }
}

/// The logo as a texture `pixels` wide, made once per size and context.
fn texture(ctx: &egui::Context, pixels: u32) -> Option<TextureHandle> {
    let id = Id::new(("sector-f-logo", pixels));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let image = rasterise(pixels)?;
    let texture = ctx.load_texture("sector-f-logo", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// The logo as a white mask `pixels` square: only the alpha comes from the
/// SVG.
pub fn rasterise(pixels: u32) -> Option<ColorImage> {
    let tree = resvg::usvg::Tree::from_str(LOGO, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels)?;
    let scale = pixels as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = pixmap
        .data()
        .chunks_exact(4)
        .flat_map(|p| [p[3], p[3], p[3], p[3]])
        .collect();
    Some(ColorImage::from_rgba_premultiplied(
        [pixels as usize, pixels as usize],
        &rgba,
    ))
}

/// Runs `command` through sh(1), detached from the bar: no stdin, and a
/// thread reaps it so it never lingers as a zombie.
fn run(command: &str) {
    match Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("patina-bar: can't run {command}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_renders_as_a_mask() {
        let image = rasterise(64).expect("logo renders");
        assert_eq!(image.size, [64, 64]);
        let alpha: Vec<u8> = image.pixels.iter().map(|p| p.a()).collect();
        let solid = alpha.iter().filter(|&&a| a == 255).count();
        assert!(solid > 300, "only {solid} solid pixels");
        assert!(alpha.contains(&0), "no background");
        // A white mask, premultiplied: every channel equals alpha.
        assert!(
            image
                .pixels
                .iter()
                .all(|p| p.r() == p.a() && p.g() == p.a() && p.b() == p.a())
        );
    }
}
