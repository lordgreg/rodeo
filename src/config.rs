//! Configuration file handling.
//!
//! rodeo reads `$XDG_CONFIG_HOME/rodeo/config.toml`, writing a default one on
//! first run. Every field has a serde default, so a partial file is valid and
//! new keys never break existing configurations.

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
};

use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::types::{ActivePane, SortOrder, SortType};

pub const CONFIG_FILENAME: &str = "config.toml";
pub const CONFIG_DIR: &str = "rodeo";

fn default_theme() -> String {
    // Must match a file in the themes directory (themes/default.toml).
    "default".to_string()
}

/// The user's home directory, looked up at *runtime*.
///
/// This used to be `env!("HOME")` — the compile-time macro — so the build
/// machine's home was baked into the binary. A distro package built in
/// `/build` started every user in `/build`, and the crate failed to compile
/// wherever `HOME` was unset (CI containers, nix, scratch images).
fn home_dir() -> String {
    home_or_root(std::env::var("HOME").ok())
}

/// The lookup rule, split out so it can be tested without mutating the
/// process environment (which would race every other test).
fn home_or_root(home: Option<String>) -> String {
    home.filter(|home| !home.is_empty())
        .unwrap_or_else(|| "/".to_string())
}

fn default_initial_directory() -> String {
    home_dir()
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
fn default_filter_gitignore() -> bool {
    true
}
fn default_filter_hidden() -> bool {
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
fn default_auto_update() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug)]
/// User configuration, read from `config.toml`.
///
/// Every field has a serde default so a partial file stays valid.
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
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    /// Show a file-type glyph before each name. Off by default: the glyphs
    /// come from a Nerd Font, and without one they render as tofu.
    #[serde(default)]
    pub icons: bool,
    /// Skip everything `.gitignore` (and `.ignore`) excludes when searching
    /// for files or in file contents.
    #[serde(default = "default_filter_gitignore")]
    pub filter_gitignore: bool,
    /// Skip dot-files and dot-directories in those same searches.
    #[serde(default = "default_filter_hidden")]
    pub filter_hidden: bool,
    /// Extra names to skip: a plain name (`target`), an extension pattern
    /// (`*.lock`), or a sub-path (`src/generated`).
    #[serde(default)]
    pub filter_entries: Vec<String>,
    /// Optional keybinding overrides: action name → key name (single,
    /// unmodified keys only). See `ui::keymap` for valid names.
    ///
    /// Must stay the last field: TOML requires every scalar value to be
    /// emitted before any table, and serialization follows declaration order.
    #[serde(default)]
    pub keybindings: HashMap<String, String>,
}

impl Default for Config {
    /// Every field above carries a serde default, so an empty document *is*
    /// the default configuration.
    ///
    /// Writing the field list out again here made a third copy that had to
    /// agree with the other two by hand — they had already fallen out of
    /// declaration order. Deserializing cannot fail while every field has a
    /// default, and `an_empty_document_is_the_default_configuration` fails
    /// loudly if one ever stops having one.
    fn default() -> Self {
        toml::from_str("").expect("every Config field must have a serde default")
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

    /// Replaces start directories that no longer exist with the home directory.
    ///
    /// Needed for more than tidiness: versions before the `env!("HOME")` fix
    /// wrote the *build machine's* home into the user's `config.toml` on first
    /// run, so a stale absolute path is already on disk for existing installs
    /// and fixing the default alone would not reach them. A directory removed
    /// between runs lands here too.
    fn repair_initial_dirs(&mut self) {
        for (side, dir) in [
            ("left", &mut self.initial_directory_left),
            ("right", &mut self.initial_directory_right),
        ] {
            if Path::new(dir.as_str()).is_dir() {
                continue;
            }

            let home = home_dir();
            warn!("initial {side} directory {dir:?} is not a directory, starting in {home}");
            *dir = home;
        }
    }

    pub fn load_config_from_file(path: &Path) -> io::Result<Config> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                // rodeo used YAML before 0.2. Point the user at their old file
                // instead of silently starting with defaults.
                let legacy = path.with_extension("yaml");
                if legacy.exists() {
                    let msg = format!(
                        "rodeo now reads {CONFIG_FILENAME}; your old {} is ignored. \
                         Convert it (key: value → key = \"value\") to keep your settings.",
                        legacy.display()
                    );
                    warn!("{msg}");
                    eprintln!("warning: {msg}");
                }

                // The file that was asked for, not the default location:
                // `--config ./new.toml` used to create the *user's* config and
                // leave `./new.toml` missing.
                warn!(
                    "Config file not found, creating default config at {}",
                    path.display()
                );

                let config = Config::default();
                Self::save_config(&config, path)?;
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

    /// Writes the configuration to `path`.
    ///
    /// Takes the path rather than an `Option` that falls back to the default
    /// location. That fallback was the bug behind `:w`: the caller already knew
    /// which file had been read, passed `None` anyway, and a session started
    /// with `--config ./rodeo.toml` wrote its settings to the user's real
    /// `config.toml` instead.
    pub fn save_config(config: &Config, path: &Path) -> io::Result<()> {
        let config_str = toml::to_string_pretty(config)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let parent_dir = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "config path has no parent directory",
            )
        })?;

        std::fs::create_dir_all(parent_dir)?;
        std::fs::write(path, config_str)?;

        info!("Config saved to {}", path.display());
        Ok(())
    }

    /// Reads the configuration at `path`, repairing start directories that no
    /// longer exist. Writes a default file when there is nothing there.
    pub fn load_config_at(path: &Path) -> io::Result<Config> {
        let mut config = Self::load_config_from_file(path)?;
        config.repair_initial_dirs();
        Ok(config)
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
        assert!(config.filter_gitignore);
        assert!(config.filter_hidden);
        assert!(config.filter_entries.is_empty());
        assert!(config.keybindings.is_empty());
    }

    #[test]
    fn an_unset_or_empty_home_still_yields_a_usable_directory() {
        assert_eq!(home_or_root(Some("/home/u".to_string())), "/home/u");
        // An unset HOME used to be a *compile* error via env!("HOME").
        assert_eq!(home_or_root(None), "/");
        assert_eq!(home_or_root(Some(String::new())), "/");
    }

    /// `env!("HOME")` is the compile-time macro: it baked the build machine's
    /// home into the binary. The lookup has to happen at run time.
    #[test]
    fn the_default_start_directory_is_read_at_run_time() {
        let Ok(home) = std::env::var("HOME") else {
            return; // Nothing to compare against in this environment.
        };
        assert_eq!(default_initial_directory(), home);
    }

    #[test]
    fn a_start_directory_that_no_longer_exists_falls_back_to_home() {
        let mut config = Config {
            initial_directory_left: "/definitely/not/a/real/directory".to_string(),
            initial_directory_right: "/tmp".to_string(),
            ..Default::default()
        };

        config.repair_initial_dirs();

        // The stale path — e.g. a build machine's home written into the config
        // file by an older rodeo — is replaced with somewhere that exists; a
        // directory that is still valid is left alone.
        assert!(
            Path::new(&config.initial_directory_left).is_dir(),
            "{:?}",
            config.initial_directory_left
        );
        assert_eq!(config.initial_directory_right, "/tmp");
    }

    #[test]
    fn deserialize_empty_toml_uses_defaults() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.theme, "default");
        assert_eq!(config.sort_type, SortType::Name);
        assert_eq!(config.sort_order, SortOrder::Ascending);
    }

    /// `Config::default` deserializes an empty document, so a field added
    /// without `#[serde(default)]` would make it panic. This is where that
    /// shows up, rather than at startup on a user's machine.
    #[test]
    fn an_empty_document_is_the_default_configuration() {
        let parsed: Result<Config, _> = toml::from_str("");
        assert!(
            parsed.is_ok(),
            "a Config field is missing #[serde(default)]: {:?}",
            parsed.err()
        );

        // And the two agree, field for field, via the serialized form.
        let from_empty = toml::to_string(&parsed.unwrap()).unwrap();
        let from_default = toml::to_string(&Config::default()).unwrap();
        assert_eq!(from_empty, from_default);
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
filter_gitignore = false
filter_hidden = false
filter_entries = ["target", "*.lock"]

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
        assert!(!config.filter_gitignore);
        assert!(!config.filter_hidden);
        assert_eq!(config.filter_entries, vec!["target", "*.lock"]);
        assert_eq!(config.keybindings.get("quit"), Some(&"Q".to_string()));
    }

    /// `--config` has to hold for the whole session. These pin the three
    /// places that used to resolve the default location afresh and so wrote to,
    /// or read from, the user's real `config.toml` instead.
    mod the_file_that_was_asked_for {
        use super::*;

        fn temp() -> tempfile::TempDir {
            tempfile::tempdir().expect("temp dir")
        }

        #[test]
        fn saving_writes_to_the_path_it_is_given() {
            let dir = temp();
            let path = dir.path().join("rodeo.toml");

            let config = Config {
                theme: "nord".to_string(),
                ..Default::default()
            };
            Config::save_config(&config, &path).unwrap();

            assert!(path.exists());
            assert!(std::fs::read_to_string(&path).unwrap().contains("nord"));
        }

        #[test]
        fn saving_creates_the_directory_it_needs() {
            let dir = temp();
            let path = dir.path().join("nested/deeper/config.toml");

            Config::save_config(&Config::default(), &path).unwrap();

            assert!(path.exists());
        }

        /// `--config ./new.toml` used to create the *user's* config file and
        /// leave `./new.toml` missing.
        #[test]
        fn a_config_that_is_not_there_is_created_where_it_was_asked_for() {
            let dir = temp();
            let path = dir.path().join("rodeo.toml");

            let config = Config::load_config_at(&path).unwrap();

            assert!(path.exists(), "the default was written to the wrong file");
            assert_eq!(config.theme, Config::default().theme);
            // And nowhere else in the directory.
            let written: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(written, ["rodeo.toml"]);
        }

        #[test]
        fn a_config_is_read_back_from_the_path_it_was_written_to() {
            let dir = temp();
            let path = dir.path().join("rodeo.toml");

            let config = Config {
                theme: "nord".to_string(),
                show_hidden: true,
                ..Default::default()
            };
            Config::save_config(&config, &path).unwrap();

            let read = Config::load_config_at(&path).unwrap();

            assert_eq!(read.theme, "nord");
            assert!(read.show_hidden);
        }

        #[test]
        fn an_explicit_config_path_is_used_verbatim() {
            assert_eq!(
                Config::get_config_path(Some("./rodeo.toml")),
                PathBuf::from("./rodeo.toml")
            );
        }

        #[test]
        fn a_malformed_config_is_an_error_rather_than_silent_defaults() {
            let dir = temp();
            let path = dir.path().join("rodeo.toml");
            std::fs::write(&path, "theme = = broken").unwrap();

            assert!(Config::load_config_at(&path).is_err());
        }
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
