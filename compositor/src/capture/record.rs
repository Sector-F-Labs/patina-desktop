//! The desktop's own screen recording (ADR-0092), from the Print key's
//! choices: the whole screen or a selection, with the pointer, saved as an
//! MP4 in ~/Videos/Screen Recordings, named as macOS names them.
//!
//! Each pass of the event loop, if the screen has changed and a thirtieth
//! of a second has gone by, the compositor draws it offscreen as captures
//! do (the indicator left out), cuts out the selection, and hands the
//! pixels to a thread that feeds `ffmpeg` on its stdin. ffmpeg stamps each
//! frame with the time it arrives and writes a steady 30 frames a second,
//! repeating the last while nothing moves. A pill at the bottom of the
//! screen (`patina-screenshot --recording`) says it's recording and stops
//! it; so does the Print key.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::time::{Duration, Instant};

use smithay::{
    backend::renderer::{
        Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, Texture,
        damage::OutputDamageTracker,
    },
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Physical, Rectangle, Size, Transform},
};

use super::Scene;
use crate::state::State;

/// The fastest it draws frames.
const FRAME: Duration = Duration::from_millis(33);
/// Frames waiting for ffmpeg before new ones are dropped.
const QUEUE: usize = 3;

/// A recording going.
pub struct Recording {
    /// What's taken, in the screen's pixels: even sizes, as the video's
    /// colour wants them.
    crop: Rectangle<i32, Physical>,
    /// The screen's size when it started; it ends if that changes.
    screen: Size<i32, Physical>,
    frames: SyncSender<Arc<Vec<u8>>>,
    last: Option<Arc<Vec<u8>>>,
    last_at: Option<Instant>,
    tracker: OutputDamageTracker,
    /// A timer is set to draw a frame held back.
    woken: bool,
    /// The indicator's stdin, closed to take it down.
    _indicator: Option<ChildStdin>,
}

/// `area` of a `screen`-sized screen at `scale`, in its pixels, cut down to
/// even sizes; all of it without an area. `None` if nothing's left.
pub fn crop_for(
    screen: Size<i32, Physical>,
    area: Option<Rectangle<i32, Logical>>,
    scale: f64,
) -> Option<Rectangle<i32, Physical>> {
    let whole = Rectangle::from_size(screen);
    let rect = match area {
        Some(area) => Rectangle::new(
            area.loc.to_physical_precise_round(scale),
            area.size.to_physical_precise_round(scale),
        )
        .intersection(whole)?,
        None => whole,
    };
    let size = Size::from((rect.size.w & !1, rect.size.h & !1));
    (size.w >= 2 && size.h >= 2).then(|| Rectangle::new(rect.loc, size))
}

/// The rows of `crop` out of `rgba`, a `width`-pixel-wide picture.
fn cut(rgba: &[u8], width: i32, crop: Rectangle<i32, Physical>) -> Vec<u8> {
    let stride = width as usize * 4;
    let (x, w) = (crop.loc.x as usize * 4, crop.size.w as usize * 4);
    (crop.loc.y..crop.loc.y + crop.size.h)
        .flat_map(|y| {
            let row = y as usize * stride + x;
            &rgba[row..row + w]
        })
        .copied()
        .collect()
}

/// ~/Videos/Screen Recordings, or wherever the user's Videos folder is.
fn folder() -> Option<PathBuf> {
    let videos = dirs::video_dir().or_else(|| Some(dirs::home_dir()?.join("Videos")))?;
    Some(videos.join("Screen Recordings"))
}

/// "Screen Recording 2026-10-25 at 17.39.12", as macOS names them.
fn name(time: chrono::DateTime<chrono::Local>) -> String {
    time.format("Screen Recording %Y-%m-%d at %H.%M.%S")
        .to_string()
}

/// `name`.mp4 in `folder`, or "`name` (2).mp4" and on if it's taken.
fn free_name(folder: &Path, name: &str) -> PathBuf {
    let mut path = folder.join(format!("{name}.mp4"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{name} ({n}).mp4"));
        n += 1;
    }
    path
}

/// ffmpeg's arguments: `size` RGBA frames on stdin, stamped as they come,
/// into an H.264 MP4 at 30 frames a second.
fn ffmpeg_args(size: Size<i32, Physical>, path: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "-video_size",
    ]
    .map(String::from)
    .to_vec();
    args.push(format!("{}x{}", size.w, size.h));
    args.extend(
        [
            "-use_wallclock_as_timestamps",
            "1",
            "-i",
            "-",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            "-pix_fmt",
            "yuv420p",
            "-fps_mode",
            "cfr",
            "-r",
            "30",
            "-movflags",
            "+faststart",
            "-y",
        ]
        .map(String::from),
    );
    args.push(path.display().to_string());
    args
}

/// Starts ffmpeg writing `path`, and a thread feeding it; frames sent to
/// what's returned go in, and dropping it ends the file.
fn start_ffmpeg(
    size: Size<i32, Physical>,
    path: PathBuf,
) -> Result<SyncSender<Arc<Vec<u8>>>, String> {
    let folder = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut child = Command::new("ffmpeg")
        .args(ffmpeg_args(size, &path))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("can't run ffmpeg: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no pipe to ffmpeg")?;
    let (frames, incoming) = sync_channel::<Arc<Vec<u8>>>(QUEUE);
    std::thread::Builder::new()
        .name("recording".into())
        .spawn(move || {
            for frame in incoming {
                if let Err(e) = stdin.write_all(&frame) {
                    eprintln!("patina-compositor: recording: {e}");
                    break;
                }
            }
            drop(stdin);
            match child.wait() {
                Ok(s) if s.success() => eprintln!("patina-compositor: saved {}", path.display()),
                Ok(s) => eprintln!("patina-compositor: recording: ffmpeg {s}"),
                Err(e) => eprintln!("patina-compositor: recording: {e}"),
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(frames)
}

impl State {
    /// Whether a recording is going.
    pub fn recording(&self) -> bool {
        self.capture.recording.is_some()
    }

    /// Starts recording `area` of the screen, or all of it.
    pub fn start_recording(&mut self, area: Option<Rectangle<i32, Logical>>) {
        if self.recording() || self.locked() {
            return;
        }
        let Some(screen) = self.output.current_mode().map(|m| m.size) else {
            return;
        };
        let scale = self.output.current_scale().fractional_scale();
        let origin = self
            .space
            .output_geometry(&self.output)
            .map(|g| g.loc)
            .unwrap_or_default();
        let area = area.map(|a| Rectangle::new(a.loc - origin, a.size));
        let Some(crop) = crop_for(screen, area, scale) else {
            eprintln!("patina-compositor: nothing to record");
            return;
        };
        let Some(folder) = folder() else {
            eprintln!("patina-compositor: recording: no home folder");
            return;
        };
        let path = free_name(&folder, &name(chrono::Local::now()));
        let frames = match start_ffmpeg(crop.size, path) {
            Ok(frames) => frames,
            Err(e) => {
                eprintln!("patina-compositor: recording: {e}");
                return;
            }
        };
        let indicator = self.show_recording_indicator();
        self.capture.recording = Some(Recording {
            crop,
            screen,
            frames,
            last: None,
            last_at: None,
            tracker: OutputDamageTracker::new(screen, scale, Transform::Normal),
            woken: false,
            _indicator: indicator,
        });
        self.needs_redraw = true;
    }

    /// Ends the recording, if one is going; the file is finished on its
    /// thread.
    pub fn stop_recording(&mut self) {
        let Some(recording) = self.capture.recording.take() else {
            return;
        };
        // The last frame again, so the video lasts until now.
        if let Some(last) = recording.last {
            let _ = recording.frames.send(last);
        }
    }

    /// Draws a frame for the recording if the screen has changed and it's
    /// time; else, if it's changed too soon, comes back when it's time.
    pub(super) fn record_frame(&mut self) {
        let Some(recording) = self.capture.recording.as_mut() else {
            return;
        };
        let wait = recording
            .last_at
            .map_or(Duration::ZERO, |at| FRAME.saturating_sub(at.elapsed()));
        if !wait.is_zero() {
            if !recording.woken {
                recording.woken = true;
                let _ =
                    self.loop_handle
                        .insert_source(Timer::from_duration(wait), |_, _, state| {
                            if let Some(recording) = state.capture.recording.as_mut() {
                                recording.woken = false;
                            }
                            TimeoutAction::Drop
                        });
            }
            return;
        }
        if self.output.current_mode().map(|m| m.size) != Some(recording.screen) {
            eprintln!("patina-compositor: the screen changed size; recording stopped");
            self.stop_recording();
            return;
        }
        let scene = Scene {
            space: &self.space,
            output: &self.output,
            shown: self.shown(),
        };
        let Some(recording) = self.capture.recording.as_mut() else {
            return;
        };
        let result = if let Some(u) = self.udev.as_mut() {
            draw::<_, smithay::backend::renderer::gles::GlesTexture>(
                &mut u.renderer,
                &scene,
                Some(&mut u.cursors),
                recording,
            )
        } else if let Some(w) = self.winit.as_mut() {
            draw::<_, smithay::backend::renderer::gles::GlesTexture>(
                w.backend.renderer(),
                &scene,
                None,
                recording,
            )
        } else if let Some(n) = self.nested.as_mut() {
            draw::<_, smithay::reexports::pixman::Image<'static, 'static>>(
                &mut n.renderer,
                &scene,
                None,
                recording,
            )
        } else {
            Ok(None)
        };
        match result {
            Ok(Some(frame)) => {
                recording.last = Some(frame.clone());
                recording.last_at = Some(Instant::now());
                match recording.frames.try_send(frame) {
                    // ffmpeg is behind: this one's dropped, the next goes.
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => {
                        eprintln!("patina-compositor: ffmpeg stopped; recording stopped");
                        self.stop_recording();
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("patina-compositor: recording: {e}");
                self.stop_recording();
            }
        }
    }
}

/// The recording's next frame, if the screen has changed since the last.
fn draw<R, T>(
    renderer: &mut R,
    scene: &Scene<'_>,
    cursors: Option<&mut crate::cursor::Cursors>,
    recording: &mut Recording,
) -> Result<Option<Arc<Vec<u8>>>, String>
where
    R: Renderer + ImportAll + ImportMem + Offscreen<T> + Bind<T> + ExportMem,
    R::TextureId: Send + Clone + Texture + 'static,
{
    let elements = scene.screen(renderer, cursors, true);
    let damaged = recording
        .tracker
        .damage_output(1, &elements)
        .map_err(|e| format!("{e:?}"))?
        .0
        .is_some();
    if !damaged && recording.last.is_some() {
        return Ok(None);
    }
    let rgba = crate::screenshot::read_back::<R, T, _>(
        renderer,
        &elements,
        recording.screen,
        scene.shown.scale,
        crate::nested::BACKGROUND,
    )?;
    Ok(Some(Arc::new(cut(
        &rgba,
        recording.screen.w,
        recording.crop,
    ))))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn named_as_macos_names_them() {
        let time = chrono::Local
            .with_ymd_and_hms(2026, 10, 25, 9, 4, 33)
            .unwrap();
        assert_eq!(name(time), "Screen Recording 2026-10-25 at 09.04.33");
    }

    #[test]
    fn the_crop_is_even_and_on_the_screen() {
        let screen = Size::from((100, 50));
        assert_eq!(
            crop_for(screen, None, 1.0),
            Some(Rectangle::from_size((100, 50).into()))
        );
        let odd = Some(Rectangle::new((10, 10).into(), (21, 13).into()));
        assert_eq!(
            crop_for(screen, odd, 1.0),
            Some(Rectangle::new((10, 10).into(), (20, 12).into()))
        );
        let at_2 = Some(Rectangle::new((5, 5).into(), (10, 10).into()));
        assert_eq!(
            crop_for(screen, at_2, 2.0),
            Some(Rectangle::new((10, 10).into(), (20, 20).into()))
        );
        let past = Some(Rectangle::new((90, 40).into(), (50, 50).into()));
        assert_eq!(
            crop_for(screen, past, 1.0),
            Some(Rectangle::new((90, 40).into(), (10, 10).into()))
        );
        let off = Some(Rectangle::new((200, 200).into(), (5, 5).into()));
        assert_eq!(crop_for(screen, off, 1.0), None);
    }

    #[test]
    fn the_selection_is_cut_out() {
        // 3x2, each pixel's red its index.
        let rgba: Vec<u8> = (0..6).flat_map(|i| [i, 0, 0, 255]).collect();
        let got = cut(&rgba, 3, Rectangle::new((1, 0).into(), (2, 2).into()));
        let reds: Vec<u8> = got.chunks(4).map(|p| p[0]).collect();
        assert_eq!(reds, [1, 2, 4, 5]);
    }

    #[test]
    fn ffmpeg_is_told_the_size_and_file() {
        let args = ffmpeg_args(Size::from((640, 480)), Path::new("/tmp/a.mp4"));
        assert!(args.windows(2).any(|w| w == ["-video_size", "640x480"]));
        assert_eq!(args.last().map(String::as_str), Some("/tmp/a.mp4"));
    }
}
