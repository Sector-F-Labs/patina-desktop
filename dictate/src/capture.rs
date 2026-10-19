//! The microphone, recorded from [`Recording::start`] until
//! [`Recording::stop`]. Linux goes through cpal (ALSA, which PipeWire and
//! PulseAudio serve); FreeBSD reads the kernel's `/dev/dsp`, there whatever
//! sound server runs, and shared with other programs by its virtual
//! channels.

use std::sync::{Arc, Mutex};

use crate::audio::Audio;

#[cfg(target_os = "linux")]
pub use linux::Recording;

/// How loud the recording is now, for the overlay's waveform; it can go to
/// another thread, which the recording itself (cpal's stream) can't.
#[derive(Clone)]
pub struct Meter {
    samples: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl Meter {
    /// The loudness (RMS) of the last thirtieth of a second.
    pub fn level(&self) -> f32 {
        let Ok(samples) = self.samples.lock() else {
            return 0.0;
        };
        let last = &samples[samples.len().saturating_sub((self.rate as usize / 30).max(1))..];
        if last.is_empty() {
            return 0.0;
        }
        (last.iter().map(|s| s * s).sum::<f32>() / last.len() as f32).sqrt()
    }
}

#[cfg(target_os = "freebsd")]
pub use freebsd::Recording;

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};

    use super::{Audio, Meter};
    use crate::audio::mono;

    pub struct Recording {
        stream: cpal::Stream,
        rate: u32,
        samples: Arc<Mutex<Vec<f32>>>,
    }

    impl Recording {
        /// Starts recording from the default input.
        pub fn start() -> Result<Self, String> {
            let device = cpal::default_host()
                .default_input_device()
                .ok_or("there's no microphone")?;
            let config = device.default_input_config().map_err(|e| e.to_string())?;
            let rate = config.sample_rate();
            let channels = usize::from(config.channels());
            let samples = Arc::new(Mutex::new(Vec::new()));
            let stream = match config.sample_format() {
                cpal::SampleFormat::I16 => build::<i16>(&device, &config, channels, &samples),
                cpal::SampleFormat::I32 => build::<i32>(&device, &config, channels, &samples),
                cpal::SampleFormat::U16 => build::<u16>(&device, &config, channels, &samples),
                cpal::SampleFormat::F32 => build::<f32>(&device, &config, channels, &samples),
                other => return Err(format!("the microphone's format {other} isn't handled")),
            }?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(Self {
                stream,
                rate,
                samples,
            })
        }

        pub fn meter(&self) -> Meter {
            Meter {
                samples: self.samples.clone(),
                rate: self.rate,
            }
        }

        /// Stops and gives what was recorded.
        pub fn stop(self) -> Audio {
            drop(self.stream);
            let samples = std::mem::take(&mut *self.samples.lock().unwrap());
            Audio {
                rate: self.rate,
                samples,
            }
        }
    }

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::SupportedStreamConfig,
        channels: usize,
        samples: &Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let samples = samples.clone();
        device
            .build_input_stream(
                &config.config(),
                move |data: &[T], _: &_| {
                    let data: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
                    samples.lock().unwrap().extend(mono(&data, channels));
                },
                |e| eprintln!("patina-dictate: the microphone: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "freebsd")]
mod freebsd {
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    use super::{Audio, Meter};
    use crate::audio::RATE;

    // sys/soundcard.h: _IOWR('P', n, int).
    const SNDCTL_DSP_SPEED: libc::c_ulong = 0xC004_5002;
    const SNDCTL_DSP_SETFMT: libc::c_ulong = 0xC004_5005;
    const SNDCTL_DSP_CHANNELS: libc::c_ulong = 0xC004_5006;
    const AFMT_S16_LE: libc::c_int = 0x10;

    pub struct Recording {
        stop: Arc<AtomicBool>,
        reader: JoinHandle<()>,
        rate: u32,
        samples: Arc<Mutex<Vec<f32>>>,
    }

    impl Recording {
        /// Starts recording from `/dev/dsp`, the default sound unit, asking
        /// for Whisper's own rate, mono, 16 bits.
        pub fn start() -> Result<Self, String> {
            let mut dsp = File::open("/dev/dsp").map_err(|e| format!("/dev/dsp: {e}"))?;
            set(&dsp, SNDCTL_DSP_SETFMT, AFMT_S16_LE)
                .filter(|&format| format == AFMT_S16_LE)
                .ok_or("/dev/dsp won't record 16-bit samples")?;
            set(&dsp, SNDCTL_DSP_CHANNELS, 1)
                .filter(|&channels| channels == 1)
                .ok_or("/dev/dsp won't record in mono")?;
            // It answers with the rate it took, which may not be the one asked.
            let rate = set(&dsp, SNDCTL_DSP_SPEED, RATE as libc::c_int)
                .filter(|&rate| rate > 0)
                .ok_or("/dev/dsp won't set a rate")? as u32;
            let stop = Arc::new(AtomicBool::new(false));
            let stopped = stop.clone();
            let samples = Arc::new(Mutex::new(Vec::new()));
            let read = samples.clone();
            let reader = std::thread::spawn(move || {
                // A thirtieth of a second at 16 kHz, so a stop is soon noticed
                // and the meter keeps up.
                let mut buffer = [0u8; 1066];
                while !stopped.load(Ordering::Relaxed) {
                    match dsp.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => read.lock().unwrap().extend(
                            buffer[..n & !1]
                                .chunks_exact(2)
                                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
                        ),
                    }
                }
            });
            Ok(Self {
                stop,
                reader,
                rate,
                samples,
            })
        }

        pub fn meter(&self) -> Meter {
            Meter {
                samples: self.samples.clone(),
                rate: self.rate,
            }
        }

        /// Stops and gives what was recorded.
        pub fn stop(self) -> Audio {
            self.stop.store(true, Ordering::Relaxed);
            let _ = self.reader.join();
            Audio {
                rate: self.rate,
                samples: std::mem::take(&mut *self.samples.lock().unwrap()),
            }
        }
    }

    /// One of OSS's set-and-answer ioctls: the value it took, if any.
    fn set(dsp: &File, request: libc::c_ulong, value: libc::c_int) -> Option<libc::c_int> {
        let mut value = value;
        // SAFETY: these requests read and write one int, which `value` is.
        let done = unsafe { libc::ioctl(dsp.as_raw_fd(), request, &mut value) };
        (done != -1).then_some(value)
    }
}
