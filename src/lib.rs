//! rodeo — a modern dual-pane terminal file manager.
//!
//! This crate exists so the internals are reachable from integration tests
//! (`tests/`). The binary in `main.rs` is a thin shell over these modules.
//!
//! - [`bookmarks`]: bookmarked paths, persisted beside the configuration.
//! - [`config`]: YAML configuration loading/saving and keybinding overrides.
//! - [`fs`]: file operation logic (copy/move/delete, size walks, transfers).
//! - [`types`]: small values shared by the configuration and the UI.
//! - [`ui`]: the ratatui application — panes, popups, dialogs, input handling.

pub mod bookmarks;
pub mod cli;
pub mod config;
pub mod fs;
pub mod logging;
pub mod types;
pub mod ui;
