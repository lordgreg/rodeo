use ratatui::{Frame, layout::Rect};

use crate::ui::{theme::Theme, uiconfig::UiConfig};

/// Trait for UI components that can render themselves into a frame area.
pub trait Component {
    /// Render the component into the given area of the frame.
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, ui: &UiConfig, area: Rect);
}
