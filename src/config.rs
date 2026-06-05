use std::path::Path;

use log::{info, warn};
use serde::{Deserialize, Serialize};
use xdg;

pub const CONFIG_FILENAME: &str = "config.yaml";
pub const CONFIG_DIR: &str = "rodeo";

fn default_read_only() -> bool {
    false
}

fn default_theme() -> String {
    "light".to_string()
}

fn default_initial_directory() -> String {
    env!("HOME").to_string()
}
////
#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_read_only")]
    read_only: bool,
    #[serde(default = "default_theme")]
    theme: String,
    #[serde(default = "default_initial_directory")]
    initial_directory: String,
}

impl Config {
    fn default() -> Self {
        Self {
            read_only: default_read_only(),
            theme: default_theme(),
            initial_directory: default_initial_directory(),
        }
    }

    pub fn initial_dir(&self) -> &String {
        &self.initial_directory
    }

    pub fn load_config_from_file(filename: &str) -> Config {
        let config_str = match std::fs::read_to_string(filename) {
            Ok(s) => s,
            Err(_) => {
                let default_config_path = Self::get_config_path(None);
                warn!(
                    "Config file not found, creating default config at {}",
                    default_config_path.to_str().unwrap()
                );

                let config = Config::default();
                Self::save_config(&config, default_config_path.to_str());
                return config;
            }
        };
        let config: Config =
            yaml_serde::from_str(&config_str).expect("Failed to parse config file");
        config
    }

    pub fn get_config_path(filename: Option<&str>) -> std::path::PathBuf {
        match filename {
            Some(filename) => Path::new(filename).to_path_buf(),
            None => xdg::BaseDirectories::with_prefix(CONFIG_DIR)
                .get_config_file(CONFIG_FILENAME)
                .expect("Failed to get config file path"),
        }
    }

    pub fn save_config(config: &Config, filename: Option<&str>) {
        let config_str = yaml_serde::to_string(config).expect("Failed to serialize config");

        let config_path = Self::get_config_path(filename);
        let parent_dir = config_path.parent().unwrap();

        std::fs::create_dir_all(parent_dir).expect("Failed to create config directory");
        std::fs::write(&config_path, config_str).expect("Failed to write config file");

        info!("Config saved to {}", &config_path.to_str().unwrap());
    }

    pub fn load_config(filename: Option<&str>) -> Config {
        Self::load_config_from_file(Self::get_config_path(filename).to_str().unwrap())
    }
}
