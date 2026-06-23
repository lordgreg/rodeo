use std::{
    io,
    path::{Path, PathBuf},
};

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::ui::{
    panes::{SortOrder, SortType},
    uiconfig::ActivePane,
};

pub const CONFIG_FILENAME: &str = "config.yaml";
pub const CONFIG_DIR: &str = "rodeo";

fn default_theme() -> String {
    "light".to_string()
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
    std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string())
}

////
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_theme")]
    theme: String,
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
}

impl Config {
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
        }
    }

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
        let config: Config = yaml_serde::from_str(&config_str).map_err(|e| {
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
                .expect("Failed to get config file path"),
        }
    }

    pub fn save_config(config: &Config, filename: Option<&str>) -> io::Result<()> {
        let config_str = yaml_serde::to_string(config)
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
            io::Error::new(io::ErrorKind::NotFound, format!("cannot get config path."))
        })?;
        Self::load_config_from_file(path)
    }
}
