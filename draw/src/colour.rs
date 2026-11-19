//! Colours as hex text: what the picker's hex field shows and takes.

use crate::bitmap::Rgba;

/// `#RRGGBB`.
pub fn hex(colour: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", colour[0], colour[1], colour[2])
}

/// A colour from `RRGGBB` or `RGB` (each digit doubled), with or without the `#`, in any case and
/// with spaces round it. Opaque. `None` for anything else, including a half-typed one.
pub fn parse_hex(text: &str) -> Option<Rgba> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let value = |s: &str| u8::from_str_radix(s, 16).ok();
    match digits.len() {
        6 => Some([value(&digits[0..2])?, value(&digits[2..4])?, value(&digits[4..6])?, 255]),
        3 => {
            let d = |i: usize| value(&digits[i..=i]).map(|v| v * 17);
            Some([d(0)?, d(1)?, d(2)?, 255])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_written_as_six_digits() {
        assert_eq!(hex([235, 40, 40, 255]), "#EB2828");
        assert_eq!(hex([0, 0, 0, 255]), "#000000");
        assert_eq!(hex([255, 255, 255, 12]), "#FFFFFF", "alpha isn't part of it");
    }

    #[test]
    fn six_and_three_digits_with_or_without_the_hash_are_read() {
        assert_eq!(parse_hex("#EB2828"), Some([235, 40, 40, 255]));
        assert_eq!(parse_hex("eb2828"), Some([235, 40, 40, 255]));
        assert_eq!(parse_hex("  #0aF "), Some([0, 170, 255, 255]), "three digits are doubled");
        assert_eq!(parse_hex("fff"), Some([255, 255, 255, 255]));
    }

    #[test]
    fn what_isnt_a_colour_is_not_one() {
        for text in ["", "#", "#12", "#1234", "#12345", "#1234567", "#GGGGGG", "red", "#12 456", "ébc"] {
            assert_eq!(parse_hex(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_colour_reads_back_as_itself() {
        for colour in [[0, 0, 0, 255], [1, 2, 3, 255], [235, 40, 40, 255], [255, 255, 255, 255]] {
            assert_eq!(parse_hex(&hex(colour)), Some(colour));
        }
    }
}
