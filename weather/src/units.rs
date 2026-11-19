//! Units for display. Forecasts are kept in metric; these convert and format.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Temperature {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speed {
    #[default]
    Kmh,
    Ms,
    Mph,
    Knots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Length {
    #[default]
    Metric,
    Imperial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Units {
    pub temperature: Temperature,
    pub speed: Speed,
    /// Millimetres, kilometres and hectopascals, or inches, miles and inches of mercury.
    pub length: Length,
}

impl Temperature {
    pub const ALL: [(Self, &str); 2] = [(Self::Celsius, "Celsius (°C)"), (Self::Fahrenheit, "Fahrenheit (°F)")];
}

impl Speed {
    pub const ALL: [(Self, &str); 4] =
        [(Self::Kmh, "km/h"), (Self::Ms, "m/s"), (Self::Mph, "mph"), (Self::Knots, "knots")];
}

impl Length {
    pub const ALL: [(Self, &str); 2] = [(Self::Metric, "mm, km, hPa"), (Self::Imperial, "in, mi, inHg")];
}

impl Units {
    /// A temperature, from Celsius.
    pub fn temp(&self, c: f32) -> f32 {
        match self.temperature {
            Temperature::Celsius => c,
            Temperature::Fahrenheit => c * 9.0 / 5.0 + 32.0,
        }
    }

    /// A rounded temperature with a degree sign, as the weather is usually written.
    pub fn degrees(&self, c: f32) -> String {
        // Avoid "-0°".
        let t = self.temp(c).round();
        format!("{}°", if t == 0.0 { 0.0 } else { t })
    }

    pub fn speed(&self, kmh: f32) -> f32 {
        match self.speed {
            Speed::Kmh => kmh,
            Speed::Ms => kmh / 3.6,
            Speed::Mph => kmh / 1.609_344,
            Speed::Knots => kmh / 1.852,
        }
    }

    pub fn speed_unit(&self) -> &'static str {
        Speed::ALL.iter().find(|(s, _)| *s == self.speed).map(|(_, n)| *n).unwrap_or("km/h")
    }

    pub fn wind(&self, kmh: f32) -> String {
        format!("{:.0} {}", self.speed(kmh), self.speed_unit())
    }

    pub fn precip(&self, mm: f32) -> f32 {
        match self.length {
            Length::Metric => mm,
            Length::Imperial => mm / 25.4,
        }
    }

    pub fn precip_unit(&self) -> &'static str {
        match self.length {
            Length::Metric => "mm",
            Length::Imperial => "in",
        }
    }

    pub fn rain(&self, mm: f32) -> String {
        match self.length {
            Length::Metric if mm < 10.0 => format!("{mm:.1} mm"),
            Length::Metric => format!("{mm:.0} mm"),
            Length::Imperial => format!("{:.2} in", mm / 25.4),
        }
    }

    /// A distance, from metres.
    pub fn distance(&self, m: f32) -> String {
        match self.length {
            Length::Metric if m < 10_000.0 => format!("{:.1} km", m / 1000.0),
            Length::Metric => format!("{:.0} km", m / 1000.0),
            Length::Imperial => format!("{:.0} mi", m / 1609.344),
        }
    }

    pub fn pressure(&self, hpa: f32) -> f32 {
        match self.length {
            Length::Metric => hpa,
            Length::Imperial => hpa * 0.029_53,
        }
    }

    pub fn pressure_unit(&self) -> &'static str {
        match self.length {
            Length::Metric => "hPa",
            Length::Imperial => "inHg",
        }
    }

    pub fn pressure_text(&self, hpa: f32) -> String {
        match self.length {
            Length::Metric => format!("{hpa:.0} hPa"),
            Length::Imperial => format!("{:.2} inHg", self.pressure(hpa)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts() {
        let us = Units { temperature: Temperature::Fahrenheit, speed: Speed::Mph, length: Length::Imperial };
        assert_eq!(us.degrees(100.0), "212°");
        assert_eq!(us.wind(16.09344), "10 mph");
        assert_eq!(us.rain(25.4), "1.00 in");
        let metric = Units::default();
        assert_eq!(metric.degrees(-0.3), "0°");
        assert_eq!(metric.rain(2.44), "2.4 mm");
        assert_eq!(metric.distance(24_000.0), "24 km");
    }
}
