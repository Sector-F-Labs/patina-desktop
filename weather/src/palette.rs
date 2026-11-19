//! The colours the app paints itself: weather icons, the temperature scale
//! and the graphs. A skin can change any of them in its `[app.weather]`
//! section; what it leaves out keeps the defaults here, which are picked to
//! read on both light and dark backgrounds.

use egui::Color32;
use patina::skin::Hex;
use serde::Deserialize;

/// `[app.weather]` in a skin. Every key is optional.
#[derive(Debug, Default, Deserialize)]
pub struct Section {
    sun: Option<Hex>,
    moon: Option<Hex>,
    cloud: Option<Hex>,
    rain: Option<Hex>,
    snow: Option<Hex>,
    lightning: Option<Hex>,
    /// The temperature scale, coldest to hottest: the range bars and the
    /// temperature graphs blend along it.
    temperature: Option<Vec<Hex>>,
    /// The UV index's scale, low to extreme.
    uv: Option<Vec<Hex>>,
    wind: Option<Hex>,
    humidity: Option<Hex>,
    pressure: Option<Hex>,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub sun: Color32,
    pub moon: Color32,
    pub cloud: Color32,
    pub rain: Color32,
    pub snow: Color32,
    pub lightning: Color32,
    /// Stops of the temperature scale, at -20, -10, 0, 10, 20, 30 and 40 °C.
    pub temperature: Vec<Color32>,
    pub uv: Vec<Color32>,
    pub wind: Color32,
    pub humidity: Color32,
    pub pressure: Color32,
}

fn hex(s: &str) -> Color32 {
    Color32::from_hex(s).unwrap_or(Color32::MAGENTA)
}

impl Palette {
    /// The defaults, with clouds and snow tuned to the background.
    pub fn new(dark: bool) -> Self {
        Self {
            sun: hex("#FFB800"),
            moon: hex(if dark { "#E8E4D0" } else { "#9A9580" }),
            cloud: hex(if dark { "#C7CCD6" } else { "#9AA3B2" }),
            rain: hex("#3B9AF5"),
            snow: hex(if dark { "#EAF2FF" } else { "#8FB4E0" }),
            lightning: hex("#FFD60A"),
            temperature: ["#6E5BD9", "#3E8EF0", "#3FC1D9", "#4CC77A", "#F2C230", "#F28A30", "#E0453A"]
                .into_iter()
                .map(hex)
                .collect(),
            uv: ["#4CC77A", "#F2C230", "#F28A30", "#E0453A", "#A54CD9"].into_iter().map(hex).collect(),
            wind: hex("#38B2A8"),
            humidity: hex("#4A90E2"),
            pressure: hex("#9B7ED9"),
        }
    }

    /// The defaults with a skin's section laid over them.
    pub fn with(mut self, s: Section) -> Self {
        let pick = |c: &mut Color32, v: Option<Hex>| {
            if let Some(Hex(v)) = v {
                *c = v;
            }
        };
        pick(&mut self.sun, s.sun);
        pick(&mut self.moon, s.moon);
        pick(&mut self.cloud, s.cloud);
        pick(&mut self.rain, s.rain);
        pick(&mut self.snow, s.snow);
        pick(&mut self.lightning, s.lightning);
        pick(&mut self.wind, s.wind);
        pick(&mut self.humidity, s.humidity);
        pick(&mut self.pressure, s.pressure);
        // A scale needs two stops to blend between.
        if let Some(t) = s.temperature.filter(|t| t.len() >= 2) {
            self.temperature = t.into_iter().map(|h| h.0).collect();
        }
        if let Some(u) = s.uv.filter(|u| !u.is_empty()) {
            self.uv = u.into_iter().map(|h| h.0).collect();
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

    pub fn uv(&self, uv: f32) -> Color32 {
        let band = match uv {
            u if u < 3.0 => 0,
            u if u < 6.0 => 1,
            u if u < 8.0 => 2,
            u if u < 11.0 => 3,
            _ => 4,
        };
        self.uv[band.min(self.uv.len() - 1)]
    }
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_ends_at_its_stops() {
        let p = Palette::new(true);
        assert_eq!(p.temp(-40.0), p.temperature[0]);
        assert_eq!(p.temp(50.0), *p.temperature.last().unwrap());
        assert_eq!(p.temp(0.0), p.temperature[2]);
    }

    #[test]
    fn a_skin_overrides_some_colours() {
        let skin = patina::skin::Skin::parse(
            "format = 1\nname = \"T\"\n[app.weather]\nsun = \"#FF0000\"\ntemperature = [\"#000000\", \"#FFFFFF\"]\n",
        )
        .unwrap();
        let (section, warnings) = skin.app_section::<Section>("weather").unwrap();
        assert!(warnings.is_empty());
        let p = Palette::new(false).with(section);
        assert_eq!(p.sun, Color32::RED);
        assert_eq!(p.temp(10.0), Color32::from_gray(128));
        assert_eq!(p.rain, hex("#3B9AF5"));
    }
}
