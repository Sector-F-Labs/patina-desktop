//! Renders `packaging/linux/reader.svg` to a square PNG:
//! `cargo run --example render_icon -- <size> <out.png>`. Used by
//! `scripts/icon.sh`, so making the icon needs no system SVG tools.

fn main() {
    let usage = "usage: render_icon <size> <out.png>";
    let mut args = std::env::args().skip(1);
    let size: u32 = args.next().and_then(|s| s.parse().ok()).expect(usage);
    let out = args.next().expect(usage);
    let svg = std::fs::read("packaging/linux/reader.svg").expect("read packaging/linux/reader.svg");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default()).expect("parse the SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).expect("a pixmap");
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.save_png(&out).expect("write the PNG");
    println!("{out}");
}
