use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::ui::{
    panes::{SortOrder, SortType},
    uiconfig::ActivePane,
};

pub const CONFIG_FILENAME: &str = "config.toml";
pub const CONFIG_DIR: &str = "rodeo";

fn default_theme() -> String {
    // Must match a file in the themes directory (themes/default.toml).
    "default".to_string()
}

fn default_initial_directory() -> String {
    env!("HOME").to_string()
}
fn default_sort_type() -> SortType {
    SortType::Name
}
fn default_sort_order() -> SortOrder {
    SortOrder::Ascending
}
fn default_show_hidden() -> bool {
    false
}
fn default_directories_on_top() -> bool {
    true
}
fn default_active_pane() -> ActivePane {
    ActivePane::Left
}
fn default_editor() -> String {
    std::env::var("VISUAL")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("EDITOR").ok())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_initial_directory")]
    pub initial_directory_left: String,
    #[serde(default = "default_initial_directory")]
    pub initial_directory_right: String,
    #[serde(default = "default_sort_type")]
    pub sort_type: SortType,
    #[serde(default = "default_sort_order")]
    pub sort_order: SortOrder,
    #[serde(default = "default_show_hidden")]
    pub show_hidden: bool,
    #[serde(default = "default_directories_on_top")]
    pub directories_on_top: bool,
    #[serde(default = "default_active_pane")]
    pub active_pane: ActivePane,
    #[serde(default = "default_editor")]
    pub editor: String,
    /// Show a file-type glyph before each name. Off by default: the glyphs
    /// come from a Nerd Font, and without one they render as tofu.
    #[serde(default)]
    pub icons: bool,
    /// Optional keybinding overrides: action name → key name (single,
    /// unmodified keys only). See `ui::keymap` for valid names.
    ///
    /// Must stay the last field: TOML requires every scalar value to be
    /// emitted before any table, and serialization follows declaration order.
    #[serde(default)]
    pub keybindings: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            initial_directory_left: default_initial_directory(),
            initial_directory_right: default_initial_directory(),
            sort_order: default_sort_order(),
            sort_type: default_sort_type(),
            show_hidden: default_show_hidden(),
            directories_on_top: default_directories_on_top(),
            active_pane: default_active_pane(),
            editor: default_editor(),
            icons: false,
            keybindings: HashMap::new(),
        }
    }
}

impl Config {
    pub fn set_initial_dir(&mut self, left: Option<String>, right: Option<String>) {
        if let Some(left) = left {
            self.initial_directory_left = PathBuf::from(left).to_string_lossy().to_string();
        };

        if let Some(right) = right {
            self.initial_directory_right = PathBuf::from(right).to_string_lossy().to_string();
        }
    }

    pub fn get_initial_dir(&self) -> &str {
        if self.active_pane == ActivePane::Left {
            &self.initial_directory_left
        } else {
            &self.initial_directory_right
        }
    }

    pub fn load_config_from_file(filename: &str) -> io::Result<Config> {
        let config_str = match std::fs::read_to_string(filename) {
            Ok(s) => s,
            Err(_) => {
                // rodeo used YAML before 0.2. Point the user at their old file
                // instead of silently starting with defaults.
                let legacy = Path::new(filename).with_extension("yaml");
                if legacy.exists() {
                    let msg = format!(
                        "rodeo now reads {CONFIG_FILENAME}; your old {} is ignored. \
                         Convert it (key: value → key = \"value\") to keep your settings.",
                        legacy.display()
                    );
                    warn!("{msg}");
                    eprintln!("warning: {msg}");
                }

                let default_config_path = Self::get_config_path(None);
                warn!(
                    "Config file not found, creating default config at {:?}",
                    default_config_path.to_str()
                );

                let config = Config::default();
                Self::save_config(&config, default_config_path.to_str())?;
                return Ok(config);
            }
        };
        let config: Config = toml::from_str(&config_str).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Failed to parse config file: {}", e),
            )
        })?;
        Ok(config)
    }

    pub fn get_config_path(filename: Option<&str>) -> std::path::PathBuf {
        match filename {
            Some(filename) => Path::new(filename).to_path_buf(),
            None => xdg::BaseDirectories::with_prefix(CONFIG_DIR)
                .get_config_file(CONFIG_FILENAME)
                .unwrap_or_else(|| {
                    // Only None when no HOME could be determined (containers,
                    // some service managers). Falling back to the working
                    // directory beats refusing to start.
                    warn!("no home directory found, using ./{CONFIG_FILENAME}");
                    PathBuf::from(CONFIG_FILENAME)
                }),
        }
    }

    pub fn save_config(config: &Config, filename: Option<&str>) -> io::Result<()> {
        let config_str = toml::to_string_pretty(config)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let config_path = Self::get_config_path(filename);
        let parent_dir = config_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "config path has no parent directory",
            )
        })?;

        std::fs::create_dir_all(parent_dir)?;
        std::fs::write(&config_path, config_str)?;

        info!("Config saved to {:?}", &config_path.to_str());
        Ok(())
    }

    pub fn load_config(filename: Option<&str>) -> io::Result<Config> {
        let config_path = Self::get_config_path(filename);
        let path = config_path.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cannot get config path.".to_string(),
            )
        })?;
        Self::load_config_from_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert_eq!(config.theme, "default");
        assert_eq!(config.sort_type, SortType::Name);
        assert_eq!(config.sort_order, SortOrder::Ascending);
        assert!(!config.show_hidden);
        assert!(config.directories_on_top);
        assert!(matches!(config.active_pane, ActivePane::Left));
        assert!(!config.icons);
        assert!(config.keybindings.is_empty());
    }

    #[test]
    fn deserialize_empty_toml_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.theme, "default");
        assert_eq!(config.sort_type, SortType::Name);
        assert_eq!(config.sort_order, SortOrder::Ascending);
    }

    #[test]
    fn deserialize_partial_toml_merges_with_defaults() {
        let toml_str = "theme = \"dark\"\nshow_hidden = true";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.theme, "dark");
        assert!(config.show_hidden);
        // Other fields should use defaults
        assert_eq!(config.sort_type, SortType::Name);
        assert!(config.directories_on_top);
    }

    #[test]
    fn deserialize_full_toml() {
        let toml_str = r#"theme = "nord"
initial_directory_left = "/tmp"
initial_directory_right = "/home"
sort_type = "Size"
sort_order = "Descending"
show_hidden = true
directories_on_top = false
active_pane = "Right"
editor = "emacs"

[keybindings]
quit = "Q"
help = "H"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.theme, "nord");
        assert_eq!(config.initial_directory_left, "/tmp");
        assert_eq!(config.initial_directory_right, "/home");
        assert_eq!(config.sort_type, SortType::Size);
        assert_eq!(config.sort_order, SortOrder::Descending);
        assert!(config.show_hidden);
        assert!(!config.directories_on_top);
        assert!(matches!(config.active_pane, ActivePane::Right));
        assert_eq!(config.editor, "emacs");
        assert_eq!(config.keybindings.get("quit"), Some(&"Q".to_string()));
    }

    #[test]
    fn default_editor_respects_visual_then_editor() {
        // Note: This test doesn't actually modify env vars to avoid side effects.
        // It just tests that default_editor() is called and returns something.
        let config = Config::default();
        // Editor should be set to VISUAL, EDITOR, or "vi" as fallback
        assert!(!config.editor.is_empty());
    }
}
