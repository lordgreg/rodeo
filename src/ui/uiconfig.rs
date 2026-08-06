//! Small pieces of UI state that several widgets need to agree on.

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}

/// Shared UI state.
///
/// Now empty: the two popup flags it used to carry were a second, drifting
/// answer to "what is open" and have been replaced by `App::overlay`. It is
/// still threaded through `Component::render`, where no implementor reads it —
/// removing that parameter is a separate cleanup.
#[derive(Debug, Default)]
pub struct UiConfig {}

impl UiConfig {
    pub fn new() -> Self {
        Self::default()
    }
}
