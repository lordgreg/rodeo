use ratatui::{Frame, layout::Rect};

use crate::ui::{theme::Theme, uiconfig::UiConfig};

/// Trait for UI components that can render themselves into a frame area.
pub trait Component {
    /// Render the component into the given area of the frame.
    fn render(&mut self, frame: &mut Frame<'_>, theme: &Theme, ui: &UiConfig, area: Rect);
}

/// A popup rectangle centered in `area`.
///
/// `want` is the size the content would like, `max` an upper bound in cells:
/// percentages alone make popups grotesque on ultrawide terminals (a 200-column
/// screen gave the keybinding list 150 columns for two columns of text). The
/// result never exceeds `area` and never shrinks below `min`, so small
/// terminals still get a usable popup.
pub fn centered_popup(area: Rect, want: (u16, u16), min: (u16, u16), max: (u16, u16)) -> Rect {
    let width = want.0.clamp(min.0, max.0).min(area.width);
    let height = want.1.clamp(min.1, max.1).min(area.height);

    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Size needed to show `lines` of text `width` cells wide inside a bordered,
/// horizontally padded block.
pub fn content_size(width: u16, lines: usize) -> (u16, u16) {
    (width.saturating_add(4), (lines as u16).saturating_add(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 200,
        height: 50,
    };

    #[test]
    fn popup_is_capped_on_wide_terminals() {
        let r = centered_popup(AREA, (180, 40), (20, 5), (80, 30));
        assert_eq!((r.width, r.height), (80, 30));
        // Centered.
        assert_eq!(r.x, (200 - 80) / 2);
        assert_eq!(r.y, (50 - 30) / 2);
    }

    #[test]
    fn popup_shrinks_to_content_when_small() {
        let r = centered_popup(AREA, (40, 9), (20, 5), (80, 30));
        assert_eq!((r.width, r.height), (40, 9));
    }

    #[test]
    fn popup_never_exceeds_the_available_area() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 30,
            height: 8,
        };
        let r = centered_popup(tiny, (100, 40), (40, 20), (120, 40));
        assert_eq!((r.width, r.height), (30, 8));
        assert_eq!((r.x, r.y), (0, 0));
    }

    #[test]
    fn content_size_adds_borders_and_padding() {
        assert_eq!(content_size(30, 7), (34, 9));
    }
}
