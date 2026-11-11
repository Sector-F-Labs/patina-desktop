//! How a web app's stored colour becomes a window's look (ADR-0133), shared so the compositor's
//! title bar and the host's border and corners agree: the frame is the site's colour, with
//! text and buttons that read on it and a border a little darker than it.

/// A colour as red, green, blue.
pub type Rgb = [u8; 3];

/// How round a web app's bottom corners are: Patina's window radius, which the compositor's
/// title bar rounds the top with. The host can't read the skin, so a skin with another radius
/// isn't followed.
pub const RADIUS: u8 = 15;

/// `#rrggbb` as a colour.
pub fn parse(hex: &str) -> Option<Rgb> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

pub fn hex([r, g, b]: Rgb) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// How light a colour looks, 0 to 1 (the WCAG relative luminance).
fn luminance([r, g, b]: Rgb) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// Dark text on a light colour, light on a dark one: whichever has more contrast.
pub fn text_on(bg: Rgb) -> Rgb {
    if luminance(bg) > 0.179 {
        [0x1f, 0x23, 0x28]
    } else {
        [0xf5, 0xf5, 0xf5]
    }
}

/// The frame's border, as Patina's Firefox theme has it: a thin line a step away from the
/// colour towards contrast, so it's lighter than a dark page and darker than a light one
/// (`#1b1b1b` gets `#3c3c3c`, `#f8f8f8` gets `#bebebe`).
pub fn border_for(bg: Rgb) -> Rgb {
    let (toward, amount) = if luminance(bg) > 0.179 {
        (0.0, 0.23)
    } else {
        (255.0, 0.14)
    };
    bg.map(|c| (f64::from(c) + (toward - f64::from(c)) * amount).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_round_trip() {
        assert_eq!(parse("#128c7e"), Some([0x12, 0x8c, 0x7e]));
        assert_eq!(hex([0x12, 0x8c, 0x7e]), "#128c7e");
        assert_eq!(parse("128c7e"), None);
        assert_eq!(parse("#12"), None);
    }

    #[test]
    fn text_reads_on_the_colour() {
        assert_eq!(
            text_on([0xf7, 0xf5, 0xf3]),
            [0x1f, 0x23, 0x28],
            "cream takes dark text"
        );
        assert_eq!(
            text_on([0x12, 0x8c, 0x7e])[0],
            0x1f,
            "WhatsApp green is light enough"
        );
        assert_eq!(
            text_on([0x10, 0x20, 0x40])[0],
            0xf5,
            "navy takes light text"
        );
    }

    #[test]
    fn the_border_is_a_step_towards_contrast() {
        // The values Patina's Firefox theme has for its dark and light windows.
        assert_eq!(border_for([0x1b, 0x1b, 0x1b]), [0x3b, 0x3b, 0x3b]);
        assert_eq!(border_for([0xf8, 0xf8, 0xf8]), [0xbf, 0xbf, 0xbf]);
        // Lighter than a dark colour, darker than a light one.
        assert!(border_for([0x1d, 0x1f, 0x1f])[0] > 0x1d);
        assert!(border_for([0xf7, 0xf5, 0xf3])[0] < 0xf7);
    }
}
