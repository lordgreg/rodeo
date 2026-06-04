#[derive(Debug, Default)]
pub enum ActivePane {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Default)]
pub struct UiConfig {
    pub active_pane: ActivePane,
    pub log_pane: bool,
    pub active_cmd_popup: bool,
    pub active_keybind_popup: bool,
    pub active_about_popup: bool,
}

impl UiConfig {
    pub fn new() -> Self {
        Self::default()
    }
}
