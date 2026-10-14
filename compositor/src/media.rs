//! The laptop's brightness and volume keys (ADR-0020). Brightness is the
//! compositor's own: it reads the backlight from /sys/class/backlight and
//! sets it through logind's SetBrightness, which lets the active session
//! change it without root, as GNOME does. Volume and the microphone run
//! commands from the `[media]` config section (PipeWire's wpctl by default).
//! Both run off the compositor's thread, so a slow call can't stall it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use smithay::input::keyboard::keysyms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    BrightnessUp,
    BrightnessDown,
    VolumeUp,
    VolumeDown,
    Mute,
    MicMute,
}

/// The media key `sym` is, if it's one.
pub fn media_key(sym: u32) -> Option<MediaKey> {
    Some(match sym {
        keysyms::KEY_XF86MonBrightnessUp => MediaKey::BrightnessUp,
        keysyms::KEY_XF86MonBrightnessDown => MediaKey::BrightnessDown,
        keysyms::KEY_XF86AudioRaiseVolume => MediaKey::VolumeUp,
        keysyms::KEY_XF86AudioLowerVolume => MediaKey::VolumeDown,
        keysyms::KEY_XF86AudioMute => MediaKey::Mute,
        keysyms::KEY_XF86AudioMicMute => MediaKey::MicMute,
        _ => return None,
    })
}

/// Carries out `key` on a thread of its own. After a volume key, the
/// volume menu flashes up on the compositor at `socket`, showing the level.
pub fn press(key: MediaKey, settings: &crate::config::Media, socket: std::ffi::OsString) {
    let settings = settings.clone();
    let _ = std::thread::Builder::new()
        .name("media-key".into())
        .spawn(move || {
            let result = match key {
                MediaKey::BrightnessUp => brightness(settings.brightness_step as i64),
                MediaKey::BrightnessDown => brightness(-(settings.brightness_step as i64)),
                MediaKey::VolumeUp => run(&settings.volume_up),
                MediaKey::VolumeDown => run(&settings.volume_down),
                MediaKey::Mute => run(&settings.mute),
                MediaKey::MicMute => run(&settings.mic_mute),
            };
            if let Err(e) = result {
                eprintln!("patina-compositor: {key:?}: {e}");
                return;
            }
            let volume_key = matches!(
                key,
                MediaKey::VolumeUp | MediaKey::VolumeDown | MediaKey::Mute
            );
            if volume_key && !settings.volume_menu.is_empty() {
                crate::clients::spawn(&socket, &format!("{} --flash", settings.volume_menu));
            }
        });
}

/// Runs `command` through sh(1); an empty command does nothing.
fn run(command: &str) -> Result<(), String> {
    if command.trim().is_empty() {
        return Ok(());
    }
    let status = Command::new("sh")
        .arg("-c")
        .arg(command)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("{command}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command}: {status}"))
    }
}

/// The backlight to change: firmware ones first (they know the panel
/// best), then platform, then raw (the GPU's, like intel_backlight).
fn backlight() -> Option<PathBuf> {
    let rank = |path: &Path| match std::fs::read_to_string(path.join("type"))
        .as_deref()
        .map(str::trim)
    {
        Ok("firmware") => 0,
        Ok("platform") => 1,
        _ => 2,
    };
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .min_by_key(|p| rank(p))
}

fn read_number(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Steps the backlight by `percent` of its range, never down to black.
fn brightness(percent: i64) -> Result<(), String> {
    let device = backlight().ok_or("no backlight")?;
    let name = device
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("odd backlight name")?
        .to_string();
    let max = read_number(&device.join("max_brightness")).ok_or("no max_brightness")?;
    let now = read_number(&device.join("brightness")).ok_or("no brightness")?;
    let value = stepped(now, max, percent);
    let system = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    let session = zbus::blocking::Proxy::new(
        &system,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .map_err(|e| e.to_string())?;
    session
        .call::<_, _, ()>("SetBrightness", &("backlight", name.as_str(), value))
        .map_err(|e| format!("logind SetBrightness: {e}"))
}

/// `now` moved by `percent` of `max`, kept between 1% of the range (never
/// black) and `max`.
pub fn stepped(now: u32, max: u32, percent: i64) -> u32 {
    let step = (max as i64 * percent.abs() / 100).max(1) * percent.signum();
    let floor = (max as i64 / 100).max(1);
    (now as i64 + step).clamp(floor, max as i64) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_steps_by_percent_of_the_range() {
        // nifl's intel_backlight: max 19393.
        assert_eq!(stepped(5953, 19393, 5), 5953 + 969);
        assert_eq!(stepped(5953, 19393, -5), 5953 - 969);
    }

    #[test]
    fn brightness_stops_at_the_ends_and_never_goes_black() {
        assert_eq!(stepped(19000, 19393, 5), 19393);
        assert_eq!(stepped(300, 19393, -5), 193);
        assert_eq!(stepped(193, 19393, -5), 193);
        // A tiny range still moves, and still keeps a floor of 1.
        assert_eq!(stepped(3, 10, 5), 4);
        assert_eq!(stepped(1, 10, -5), 1);
    }

    #[test]
    fn media_keys_are_recognised() {
        assert_eq!(
            media_key(keysyms::KEY_XF86MonBrightnessUp),
            Some(MediaKey::BrightnessUp)
        );
        assert_eq!(media_key(keysyms::KEY_XF86AudioMute), Some(MediaKey::Mute));
        assert_eq!(media_key(keysyms::KEY_a), None);
    }

    #[test]
    fn an_empty_command_does_nothing() {
        assert_eq!(run("  "), Ok(()));
        assert!(run("exit 3").is_err());
    }
}
