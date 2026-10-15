//! The laptop's brightness and volume keys (ADR-0020). Brightness steps the
//! backlight through logind (patina-shell's `backlight`, shared with the
//! volume menu's slider). Volume and the microphone run commands from the
//! `[media]` config section (PipeWire's wpctl by default). Both run off the
//! compositor's thread, so a slow call can't stall it.

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

/// Carries out `key` on a thread of its own. After a volume or brightness
/// key, the volume menu flashes up on the compositor at `socket`, showing
/// the level it changed (ADR-0033).
pub fn press(key: MediaKey, settings: &crate::config::Media, socket: std::ffi::OsString) {
    let settings = settings.clone();
    let _ = std::thread::Builder::new()
        .name("media-key".into())
        .spawn(move || {
            let step = settings.brightness_step as i64;
            let result = match key {
                MediaKey::BrightnessUp => patina_shell::backlight::step(step),
                MediaKey::BrightnessDown => patina_shell::backlight::step(-step),
                MediaKey::VolumeUp => run(&settings.volume_up),
                MediaKey::VolumeDown => run(&settings.volume_down),
                MediaKey::Mute => run(&settings.mute),
                MediaKey::MicMute => run(&settings.mic_mute),
            };
            if let Err(e) = result {
                eprintln!("patina-compositor: {key:?}: {e}");
                return;
            }
            let flash = match key {
                MediaKey::VolumeUp | MediaKey::VolumeDown | MediaKey::Mute => "--flash",
                MediaKey::BrightnessUp | MediaKey::BrightnessDown => "--brightness",
                MediaKey::MicMute => return,
            };
            if !settings.volume_menu.is_empty() {
                crate::clients::spawn(&socket, &format!("{} {flash}", settings.volume_menu));
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

#[cfg(test)]
mod tests {
    use super::*;

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
