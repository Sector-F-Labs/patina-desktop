//! Desktop pieces in egui: a bar, a launcher, a menu or a lock screen are
//! egui UIs on wlr-layer-shell surfaces, drawn in software. This crate is
//! what they share: the host that makes and drives the surfaces
//! ([`Host`], ADR-0003 and ADR-0011 of the Patina desktop), the software
//! painter ([`raster`], its ADR-0002), one running copy per display
//! ([`piece`]), a text notice ([`toast`]), the Patina bar's skin and edge ([`hud`]), and the open
//! windows ([`toplevels`]); drags out to other apps ([`drag`]).

pub mod drag;
pub mod gaps;
pub mod host;
pub mod hud;
pub mod piece;
pub mod raster;
mod scale;
mod shadow;
pub mod toast;
pub mod toplevels;

pub use drag::{Data, Drag, Icon};
pub use host::{Anchor, Host, KeyboardInteractivity, Layer, Options, Shell, output_size};
pub use shadow::Style as ShadowStyle;
pub use smithay_client_toolkit::reexports::calloop;
pub use toplevels::{Toplevel, ToplevelRequest};
