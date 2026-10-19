//! Sound as Whisper takes it: 16 kHz mono, as f32 from -1 to 1. A WAV file
//! or the microphone is brought to that here.

use std::path::Path;

/// The rate Whisper's models are trained on.
pub const RATE: u32 = 16_000;

/// Mono samples at some rate.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Audio {
    pub rate: u32,
    pub samples: Vec<f32>,
}

impl Audio {
    /// The same sound at [`RATE`].
    pub fn at_whisper_rate(self) -> Vec<f32> {
        resample(&self.samples, self.rate, RATE)
    }

    /// The loudness (RMS) of the loudest twentieth of a second.
    pub fn loudest(&self) -> f32 {
        let window = (self.rate as usize / 20).max(1);
        self.samples
            .chunks(window)
            .map(|chunk| (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len() as f32).sqrt())
            .fold(0.0, f32::max)
    }

    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / self.rate.max(1) as f32
    }
}

/// Reads a WAV file of any rate, channels and sample format.
pub fn read_wav(path: &Path) -> Result<Audio, hound::Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let full = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| s as f32 / full))
                .collect::<Result<_, _>>()?
        }
    };
    Ok(Audio {
        rate: spec.sample_rate,
        samples: mono(&interleaved, spec.channels.into()),
    })
}

/// The average of each frame's channels.
pub fn mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// From one rate to another. Going down, each new sample is the average of
/// the old ones it covers, which keeps most of what's above the new
/// rate's Nyquist out; going up, it's interpolated. Plenty for speech.
pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 || to == 0 {
        return samples.to_vec();
    }
    let step = from as f64 / to as f64;
    let len = (samples.len() as f64 / step).floor() as usize;
    (0..len)
        .map(|i| {
            let start = i as f64 * step;
            if step > 1.0 {
                let first = start as usize;
                let end = (((i + 1) as f64 * step) as usize).clamp(first + 1, samples.len());
                samples[first..end].iter().sum::<f32>() / (end - first) as f32
            } else {
                let first = start as usize;
                let next = (first + 1).min(samples.len() - 1);
                let t = (start - first as f64) as f32;
                samples[first] * (1.0 - t) + samples[next] * t
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudest_is_the_loudest_window() {
        let mut samples = vec![0.01; 40];
        samples[20..30].fill(-0.5);
        let audio = Audio { rate: 200, samples };
        assert_eq!(audio.loudest(), 0.5);
        assert_eq!(Audio::default().loudest(), 0.0);
    }

    #[test]
    fn mono_averages_each_frame() {
        assert_eq!(mono(&[1.0, 0.0, -0.5, 0.5], 2), vec![0.5, 0.0]);
        assert_eq!(mono(&[0.25, 0.5], 1), vec![0.25, 0.5]);
    }

    #[test]
    fn down_to_a_third_averages_threes() {
        let from_48k = [0.0, 0.5, 1.0, 1.0, 1.0, 1.0, 0.5];
        assert_eq!(resample(&from_48k, 48_000, 16_000), vec![0.5, 1.0]);
    }

    #[test]
    fn keeps_the_length_in_time() {
        let second = vec![0.0; 44_100];
        assert_eq!(resample(&second, 44_100, RATE).len(), 16_000);
        assert_eq!(resample(&vec![0.0; 8_000], 8_000, RATE).len(), 16_000);
    }

    #[test]
    fn up_interpolates() {
        assert_eq!(resample(&[0.0, 1.0], 8_000, 16_000), vec![0.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn the_same_rate_is_left_alone() {
        assert_eq!(resample(&[0.1, 0.2], RATE, RATE), vec![0.1, 0.2]);
    }
}
