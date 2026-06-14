#[derive(Debug, Default, PartialEq)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Default)]
pub struct UiConfig {
    // pub log_pane: bool,
    // pub active_cmd_popup: bool,
    pub active_keybind_popup: bool,
    pub active_about_popup: bool,
}

impl UiConfig {
    pub fn new() -> Self {
        Self::default()
    }
}
