//! mdviewer: a fast, read-only Markdown viewer with automatic dialect detection.

pub mod assets;
pub mod context;
pub mod dialect;
pub mod feature;
pub mod history;
pub mod links;
pub mod logging;
pub mod network;
pub mod obsidian_vaults;
pub mod page;
pub mod paths;
pub mod platform;
pub mod render;
pub mod settings;
pub mod site;
pub mod viewer;
pub mod wiki;

#[cfg(test)]
pub(crate) mod test_util;
