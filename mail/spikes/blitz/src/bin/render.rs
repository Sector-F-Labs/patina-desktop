//! `render <in.html> <out.png> [width]`: renders a message to a PNG and
//! reports what the guard served and refused, and how long each step took.

use std::collections::HashMap;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let (input, output) = (args.next().expect("in.html"), args.next().expect("out.png"));
    let width: u32 = args.next().and_then(|w| w.parse().ok()).unwrap_or(700);
    let html = std::fs::read_to_string(&input).expect("read the HTML");
    let mut parts = HashMap::new();
    parts.insert("chart@acme".to_string(), std::fs::read("../../assets/mail-icon-128.png").unwrap().into());

    let t = Instant::now();
    let mut message = blitz_spike::Message::new(&html, parts, width, 1.0, false);
    let parsed = t.elapsed();
    let height = message.height().ceil() as u32;
    let t = Instant::now();
    let rgba = message.paint(width, 0.0, height, 1.0);
    let painted = t.elapsed();
    image::RgbaImage::from_raw(width, height, rgba).expect("buffer size").save(&output).expect("save");

    println!("{input}: {width}x{height}, parse+style+layout {parsed:.1?}, paint {painted:.1?}");
    for u in message.guard.served.lock().unwrap().iter() {
        println!("  served  {u}");
    }
    for u in message.guard.refused.lock().unwrap().iter() {
        println!("  refused {u}");
    }
}
