use crate::ui::textinput::TextInput;

/// Active filter applied to a pane's entry list.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterSpec {
    Fuzzy(String),
    Regex(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchKind {
    Fuzzy,
    Regex,
}

/// State of the search/filter input bar while it is being edited.
#[derive(Debug)]
pub struct Search {
    pub kind: SearchKind,
    pub input: TextInput,
    pub regex_invalid: bool,
}

impl Search {
    pub fn fuzzy() -> Self {
        Self {
            kind: SearchKind::Fuzzy,
            input: TextInput::default(),
            regex_invalid: false,
        }
    }

    pub fn regex(initial: String) -> Self {
        Self {
            kind: SearchKind::Regex,
            input: TextInput::new(initial),
            regex_invalid: false,
        }
    }
}
