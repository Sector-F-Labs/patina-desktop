//! The colours the graphs paint: defaults that read on light and dark backgrounds, which a skin can
//! change in its `[app.home]` section. What a skin leaves out keeps the defaults here.

use egui::Color32;
use patina::skin::Hex;
use serde::Deserialize;

/// `[app.home]` in a skin. Every key is optional.
#[derive(Debug, Default, Deserialize)]
pub struct Section {
    /// The temperature scale, coldest to hottest, over -20 to 40 °C.
    temperature: Option<Vec<Hex>>,
    power: Option<Hex>,
    humidity: Option<Hex>,
    /// Any other sensor.
    line: Option<Hex>,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub temperature: Vec<Color32>,
    pub power: Color32,
    pub humidity: Color32,
    pub line: Color32,
}

fn hex(s: &str) -> Color32 {
    Color32::from_hex(s).unwrap_or(Color32::MAGENTA)
}

impl Palette {
    pub fn new() -> Self {
        Self {
            temperature: ["#6E5BD9", "#3E8EF0", "#3FC1D9", "#4CC77A", "#F2C230", "#F28A30", "#E0453A"]
                .into_iter()
                .map(hex)
                .collect(),
            power: hex("#F2B134"),
            humidity: hex("#4A90E2"),
            line: hex("#38B2A8"),
        }
    }

    /// The defaults with a skin's section laid over them.
    pub fn with(mut self, s: Section) -> Self {
        let pick = |c: &mut Color32, v: Option<Hex>| {
            if let Some(Hex(v)) = v {
                *c = v;
            }
        };
        pick(&mut self.power, s.power);
        pick(&mut self.humidity, s.humidity);
        pick(&mut self.line, s.line);
        // A scale needs two stops to blend between.
        if let Some(t) = s.temperature.filter(|t| t.len() >= 2) {
            self.temperature = t.into_iter().map(|h| h.0).collect();
        }
        self
    }

    /// The colour of a temperature in Celsius, blended along the scale.
    pub fn temp(&self, c: f32) -> Color32 {
        let stops = &self.temperature;
        // The stops span -20..40 °C however many there are.
        let at = ((c + 20.0) / 60.0).clamp(0.0, 1.0) * (stops.len() - 1) as f32;
        let i = (at.floor() as usize).min(stops.len() - 2);
        lerp(stops[i], stops[i + 1], at - i as f32)
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_ends_at_its_stops() {
        let p = Palette::new();
        assert_eq!(p.temp(-40.0), p.temperature[0]);
        assert_eq!(p.temp(50.0), *p.temperature.last().unwrap());
        assert_eq!(p.temp(0.0), p.temperature[2]);
    }

    #[test]
    fn a_skin_overrides_some_colours() {
        let skin = patina::skin::Skin::parse(
            "format = 1\nname = \"T\"\n[app.home]\npower = \"#FF0000\"\ntemperature = [\"#000000\", \"#FFFFFF\"]\n",
        )
        .unwrap();
        let (section, warnings) = skin.app_section::<Section>("home").unwrap();
        assert!(warnings.is_empty());
        let p = Palette::new().with(section);
        assert_eq!(p.power, Color32::RED);
        assert_eq!(p.temp(10.0), Color32::from_gray(128));
        assert_eq!(p.humidity, hex("#4A90E2"));
    }
}
