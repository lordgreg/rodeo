//! Which pane has focus.
//!
//! This module used to hold a `UiConfig` struct as well, carrying two booleans
//! that said whether the preview and keybinds popups were open. They were a
//! second answer to a question `App::overlay` already answers, and they were
//! threaded through every `Component::render` signature without a single
//! implementor reading them. Both are gone.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}
