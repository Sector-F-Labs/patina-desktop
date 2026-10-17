//! What the Patina desktop's pieces read and change on the machine, and
//! draw, beyond the generic layer-shell host in patina-shell: the volume
//! (`audio`, through wpctl), Wi-Fi (`network`, NetworkManager), the battery
//! (`power`), the backlight (`backlight`, which the compositor uses too),
//! the tray's icons (`icons`), the Sector F flask and lettering (`logo`),
//! Night Light's controls on the compositor (`night_light`), and power modes
//! through power-profiles-daemon (`power_profiles`), and the processes the
//! compositor starts (`processes`).

pub mod audio;
pub mod backlight;
pub mod icons;
pub mod logo;
pub mod network;
pub mod night_light;
pub mod power;
pub mod power_profiles;
pub mod processes;
