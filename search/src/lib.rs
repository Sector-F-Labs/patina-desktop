//! File search for the launcher (ADR-0137): which folders to search and how,
//! in `~/.config/patina-search/config.toml`, which Settings' Search page
//! edits; and the runs of fd and ripgrep that find files by name and by what
//! is in them. The launcher and Settings share this crate, so what the page
//! offers is what the launcher does.

pub mod config;
pub mod engine;

pub use config::{Config, Folder, Style};
pub use engine::{Hit, Tools};
