//! Small value types shared by the configuration and the UI.
//!
//! These three enums are *persisted*: they are fields of [`crate::config`]'s
//! `Config`, so they are serialised to `config.toml` and read back on start.
//! They are also *interpreted*: `ui::panes` sorts by them and `ui` decides
//! which pane has focus from them.
//!
//! That made them a cycle. They were defined in `ui::panes` and
//! `ui::uiconfig`, so `config` had to reach up into `ui` to name its own
//! fields, while `ui` reached back down into `config` to read the settings —
//! the two layers could not be understood, or compiled in a test, apart.
//!
//! Neither layer owns a sort column or a pane side; the file format does.
//! So they live here, below both, and depend on nothing but serde.

use serde::{Deserialize, Serialize};

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone, Copy)]
/// Column the listing is ordered by.
pub enum SortType {
    Flagged,
    Name,
    Size,
    Time,
}

impl SortType {
    /// The next column in the rotation, wrapping.
    pub fn next(self) -> Self {
        match self {
            Self::Flagged => Self::Name,
            Self::Name => Self::Size,
            Self::Size => Self::Time,
            Self::Time => Self::Flagged,
        }
    }

    /// The previous column in the rotation, wrapping.
    pub fn prev(self) -> Self {
        match self {
            Self::Flagged => Self::Time,
            Self::Time => Self::Size,
            Self::Size => Self::Name,
            Self::Name => Self::Flagged,
        }
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone, Copy)]
/// Direction of the active sort.
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

/// Which pane has focus.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize, Copy, Clone)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sort_rotation_is_a_cycle_in_both_directions() {
        let mut seen = vec![SortType::Name];
        let mut sort = SortType::Name;
        for _ in 0..4 {
            sort = sort.next();
            seen.push(sort);
        }
        assert_eq!(sort, SortType::Name, "next must wrap back round");
        assert_eq!(seen.len(), 5);

        for _ in 0..4 {
            sort = sort.prev();
        }
        assert_eq!(sort, SortType::Name, "prev must wrap back round");
    }

    #[test]
    fn next_and_prev_undo_each_other() {
        for sort in [
            SortType::Flagged,
            SortType::Name,
            SortType::Size,
            SortType::Time,
        ] {
            assert_eq!(sort.next().prev(), sort);
            assert_eq!(sort.prev().next(), sort);
        }
    }

    #[test]
    fn reversing_the_order_twice_is_the_identity() {
        assert_eq!(SortOrder::Ascending.reversed(), SortOrder::Descending);
        assert_eq!(
            SortOrder::Descending.reversed().reversed(),
            SortOrder::Descending
        );
    }

    /// These types are written to `config.toml`; the names in the file are
    /// part of the format and must not drift.
    #[test]
    fn the_persisted_names_are_stable() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Persisted {
            sort_type: SortType,
            sort_order: SortOrder,
            active_pane: ActivePane,
        }

        let value = Persisted {
            sort_type: SortType::Time,
            sort_order: SortOrder::Descending,
            active_pane: ActivePane::Right,
        };
        let text = toml::to_string(&value).unwrap();

        assert!(text.contains(r#"sort_type = "Time""#), "{text}");
        assert!(text.contains(r#"sort_order = "Descending""#), "{text}");
        assert!(text.contains(r#"active_pane = "Right""#), "{text}");
        assert_eq!(toml::from_str::<Persisted>(&text).unwrap(), value);
    }
}
