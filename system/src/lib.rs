//! What the Patina desktop's pieces read and change on the machine, and
//! draw, beyond the generic layer-shell host in patina-shell: the volume
//! (`audio`, through wpctl), Wi-Fi (`network`, NetworkManager), the battery
//! (`power`), the backlight (`backlight`, which the compositor uses too),
//! the tray's icons (`icons`), the Sector F flask and lettering (`logo`),
//! Night Light's controls on the compositor (`night_light`), keyboard layouts
//! (`keyboard`), default apps (`defaults`), and power modes
//! through power-profiles-daemon (`power_profiles`), the processes the
//! compositor starts (`processes`), locking, logging out, restarting and
//! shutting down (`session`), the compositor's tiling (`tiling`), and the
//! apps pinned to the bar and its tabs' order (`pins`).

pub mod apps;
pub mod audio;
pub mod backlight;
pub mod defaults;
pub mod icons;
pub mod keyboard;
pub mod logo;
pub mod network;
pub mod night_light;
pub mod pins;
pub mod power;
pub mod power_profiles;
pub mod processes;
pub mod session;
pub mod sni;
pub mod tiling;
