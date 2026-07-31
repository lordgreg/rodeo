use std::fs;
use std::io;
use std::io::Cursor;
use std::io::Error;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use log::info;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use syntect::highlighting::Theme as SynTheme;
use syntect::highlighting::ThemeSet;

use crate::config::CONFIG_DIR;

pub const DEFAULT_THEME_FILENAME: &str = "default.toml";
pub const DEFAULT_THEME_NAME: &str = "default";
/// Directory holding themes, relative to a data directory (or the working
/// directory when running from a checkout).
pub const THEME_SUBDIR: &str = "themes";

/// Compiled-in copy of the bundled default theme. It guarantees rodeo can
/// always start with a sane palette — even when no theme files are installed
/// anywhere on the system.
const BUILTIN_DEFAULT_THEME: &str = include_str!("../../themes/default.toml");

/// Search path for theme files, most specific first:
/// 1. `$XDG_DATA_HOME/rodeo/themes` — the user's own themes,
/// 2. `$XDG_DATA_DIRS/rodeo/themes` (e.g. `/usr/share/rodeo/themes`) — packaged,
/// 3. `./themes` — running from a source checkout.
pub fn theme_dirs() -> Vec<PathBuf> {
    let xdg = xdg::BaseDirectories::with_prefix(CONFIG_DIR);
    build_theme_dirs(xdg.get_data_home(), xdg.get_data_dirs())
}

/// Pure part of [`theme_dirs`], separated so it can be tested without
/// touching the process environment.
fn build_theme_dirs(data_home: Option<PathBuf>, data_dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = data_home
        .into_iter()
        .chain(data_dirs)
        .map(|d| d.join(THEME_SUBDIR))
        .collect();
    dirs.push(PathBuf::from(THEME_SUBDIR));
    dirs.dedup();
    dirs
}

/// First existing `<dir>/<name>.toml` along the search path.
fn find_theme_file(name: &str) -> Option<PathBuf> {
    theme_dirs()
        .into_iter()
        .map(|dir| dir.join(format!("{name}.toml")))
        .find(|path| path.is_file())
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Colors {
    background: String,
    foreground: String,
    primary: String,
    secondary: String,
    success: String,
    warning: String,
    error: String,
    info: String,
    muted: String,
    border: String,
    surface: String,
    highlight: String,
    accent1: String,
    accent2: String,
    accent3: String,
}

trait ColorExt {
    fn hex_to_color(hex: &str) -> Color;
}

impl ColorExt for Color {
    fn hex_to_color(hex: &str) -> Color {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        Color::Rgb(r, g, b)
    }
}

impl Colors {
    pub fn background(&self) -> Color {
        Color::hex_to_color(&self.background)
    }

    pub fn foreground(&self) -> Color {
        Color::hex_to_color(&self.foreground)
    }

    pub fn primary(&self) -> Color {
        Color::hex_to_color(&self.primary)
    }

    pub fn secondary(&self) -> Color {
        Color::hex_to_color(&self.secondary)
    }

    pub fn success(&self) -> Color {
        Color::hex_to_color(&self.success)
    }

    pub fn warning(&self) -> Color {
        Color::hex_to_color(&self.warning)
    }

    pub fn error(&self) -> Color {
        Color::hex_to_color(&self.error)
    }

    pub fn info(&self) -> Color {
        Color::hex_to_color(&self.info)
    }

    pub fn muted(&self) -> Color {
        Color::hex_to_color(&self.muted)
    }

    pub fn border(&self) -> Color {
        Color::hex_to_color(&self.border)
    }

    pub fn surface(&self) -> Color {
        Color::hex_to_color(&self.surface)
    }

    pub fn highlight(&self) -> Color {
        Color::hex_to_color(&self.highlight)
    }

    pub fn accent1(&self) -> Color {
        Color::hex_to_color(&self.accent1)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub colors: Colors,
}

impl Theme {
    pub fn to_syntect_theme(&self) -> SynTheme {
        let colors = &self.colors;

        let output = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
        <key>name</key>
        <string>{name}</string>
        <key>author</key>
        <string>rodeo</string>
        <key>settings</key>
        <array>
            <dict>
                <key>settings</key>
                <dict>
                    <key>background</key>
                    <string>{bg}</string>
                    <key>foreground</key>
                    <string>{fg}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Text</string>
                <key>scope</key>
                <string>source, text, variable, variable.other, variable.other.member,
                    variable.function, punctuation.definition, punctuation.section,
                    punctuation.terminator, punctuation.accessor</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{fg}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Comment</string>
                <key>scope</key>
                <string>comment, comment.line, comment.line.double-slash,
                    comment.line.double-dash, comment.line.number-sign, comment.block,
                    comment.block.documentation, punctuation.definition.comment,
                    meta.documentation</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{muted}</string>
                    <key>fontStyle</key>
                    <string>italic</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Punctuation</string>
                <key>scope</key>
                <string>punctuation.separator, punctuation.separator.comma,
                    punctuation.separator.colon, punctuation.separator.semicolon,
                    punctuation.separator.dot-access, markup.quote,
                    markup.link.url</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{muted}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Keyword</string>
                <key>scope</key>
                <string>keyword, keyword.other, keyword.other.unit</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{primary}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Keyword Control</string>
                <key>scope</key>
                <string>keyword.control, keyword.control.flow, keyword.control.conditional,
                    keyword.control.import, keyword.control.exception,
                    keyword.control.return</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{primary}</string>
                    <key>fontStyle</key>
                    <string>bold</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Operator</string>
                <key>scope</key>
                <string>keyword.operator, keyword.operator.assignment,
                    keyword.operator.arithmetic, keyword.operator.logical,
                    keyword.operator.comparison, entity.name.tag,
                    entity.name.label</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{secondary}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>String</string>
                <key>scope</key>
                <string>string, string.quoted, string.quoted.double, string.quoted.single,
                    string.quoted.triple, string.quoted.raw, string.regexp, string.other,
                    markup.raw, markup.raw.block, markup.inserted</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{success}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Storage Modifier</string>
                <key>scope</key>
                <string>storage, storage.modifier, storage.modifier.lifetime,
                    storage.modifier.mut</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{warning}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Storage Type</string>
                <key>scope</key>
                <string>storage.type, storage.type.class, storage.type.struct,
                    storage.type.enum, storage.type.trait, storage.type.function</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{warning}</string>
                    <key>fontStyle</key>
                    <string>bold</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Invalid</string>
                <key>scope</key>
                <string>invalid, invalid.illegal, invalid.deprecated, markup.deleted</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{error}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Support</string>
                <key>scope</key>
                <string>support.function, support.function.builtin, support.function.macro,
                    support.type, support.type.builtin, support.class,
                    support.class.builtin, support.module, support.constant, markup.link,
                    markup.link.text, markup.list, markup.list.numbered,
                    markup.list.unnumbered</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{info}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Entity</string>
                <key>scope</key>
                <string>entity.name.function, entity.name.section,
                    entity.other.attribute-name, entity.other.inherited-class</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{highlight}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Entity Type</string>
                <key>scope</key>
                <string>entity.name.type, entity.name.type.class, entity.name.type.struct,
                    entity.name.type.enum, entity.name.type.trait,
                    entity.name.type.interface, markup.heading, markup.heading.1,
                    markup.heading.2, markup.heading.3, markup.heading.4,
                    markup.heading.5, markup.heading.6</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{highlight}</string>
                    <key>fontStyle</key>
                    <string>bold</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Constant</string>
                <key>scope</key>
                <string>constant.numeric, constant.numeric.float, constant.numeric.integer,
                    constant.language, constant.language.boolean,
                    variable.language</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{accent1}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Character</string>
                <key>scope</key>
                <string>constant.character, constant.character.escape, constant.other,
                    constant.other.placeholder</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{accent2}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Parameter</string>
                <key>scope</key>
                <string>variable.parameter, variable.parameter.function,
                    entity.name.function.preprocessor, meta.annotation,
                    meta.annotation.identifier, meta.preprocessor</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{accent3}</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Emphasis</string>
                <key>scope</key>
                <string>markup.italic, markup.underline, markup.strikethrough</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{accent3}</string>
                    <key>fontStyle</key>
                    <string>italic</string>
                </dict>
            </dict>
            <dict>
                <key>name</key>
                <string>Strong</string>
                <key>scope</key>
                <string>markup.bold</string>
                <key>settings</key>
                <dict>
                    <key>foreground</key>
                    <string>{accent3}</string>
                    <key>fontStyle</key>
                    <string>bold</string>
                </dict>
            </dict>
        </array>
    </dict>
</plist>
"#,
            name = self.name,
            bg = colors.background,
            fg = colors.foreground,
            muted = colors.muted,
            primary = colors.primary,
            secondary = colors.secondary,
            success = colors.success,
            warning = colors.warning,
            error = colors.error,
            info = colors.info,
            highlight = colors.highlight,
            accent1 = colors.accent1,
            accent2 = colors.accent2,
            accent3 = colors.accent3,
        );

        let mut cursor = Cursor::new(output);
        ThemeSet::load_from_reader(&mut cursor).expect("failed to load generated theme")
    }

    /// Loads a theme by name (looked up along [`theme_dirs`]) or by path when
    /// the argument ends in `.toml`. `None` loads the default theme.
    ///
    /// Never fatal: an unknown name falls back to the default theme, and a
    /// default that is not installed anywhere falls back to the compiled-in
    /// copy.
    pub fn load_theme(name: Option<&str>) -> io::Result<Self> {
        let name = name.unwrap_or(DEFAULT_THEME_NAME);

        if name.ends_with(".toml") {
            return Self::load_from_file(Path::new(name));
        }

        if let Some(path) = find_theme_file(name) {
            return Self::load_from_file(&path);
        }

        if name != DEFAULT_THEME_NAME {
            log::warn!(
                "theme '{name}' not found in {:?}, using default",
                theme_dirs()
            );
            if let Some(path) = find_theme_file(DEFAULT_THEME_NAME) {
                return Self::load_from_file(&path);
            }
        }

        log::warn!(
            "no theme files found in {:?}, using built-in theme",
            theme_dirs()
        );
        Self::builtin()
    }

    /// The compiled-in default theme.
    pub fn builtin() -> io::Result<Self> {
        Self::from_str(BUILTIN_DEFAULT_THEME)
    }

    /// Every theme name found along the search path, plus the built-in one,
    /// de-duplicated (earlier directories win) and sorted.
    pub fn get_theme_list() -> Vec<String> {
        let mut themes: Vec<String> = vec![DEFAULT_THEME_NAME.to_string()];

        for dir in theme_dirs() {
            let entries = match fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(e) => {
                    log::debug!("skipping theme directory {}: {e}", dir.display());
                    continue;
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();

                if path.extension().is_none_or(|ext| ext != "toml") {
                    continue;
                }

                if let Some(stem) = path.file_stem()
                    && let Some(name) = stem.to_str()
                    && !themes.iter().any(|t| t == name)
                {
                    themes.push(name.to_string());
                }
            }
        }

        themes.sort();
        themes
    }

    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let theme_str = std::fs::read_to_string(path).map_err(|e| {
            log::error!("cannot read theme file '{}': {e}", path.display());
            e
        })?;

        let theme = Self::from_str(&theme_str)?;
        info!("Loaded theme {} from {}", theme.name, path.display());
        Ok(theme)
    }

    fn from_str(theme_str: &str) -> io::Result<Self> {
        toml::from_str(theme_str).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("cannot parse theme data: {e}"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_dirs_are_ordered_user_system_local() {
        let dirs = build_theme_dirs(
            Some(PathBuf::from("/home/u/.local/share/rodeo")),
            vec![
                PathBuf::from("/usr/local/share/rodeo"),
                PathBuf::from("/usr/share/rodeo"),
            ],
        );

        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/home/u/.local/share/rodeo/themes"),
                PathBuf::from("/usr/local/share/rodeo/themes"),
                PathBuf::from("/usr/share/rodeo/themes"),
                PathBuf::from("themes"),
            ]
        );
    }

    #[test]
    fn theme_dirs_always_include_the_working_directory() {
        // No HOME and no XDG_DATA_DIRS: the checkout-relative path remains.
        let dirs = build_theme_dirs(None, Vec::new());
        assert_eq!(dirs, vec![PathBuf::from("themes")]);
    }

    #[test]
    fn builtin_theme_is_valid_and_parses() {
        let theme = Theme::builtin().expect("compiled-in default theme must parse");
        assert!(!theme.name.is_empty());
        // Sanity: colors resolve to real RGB values, not the fallback.
        assert!(matches!(theme.colors.background(), Color::Rgb(_, _, _)));
    }

    #[test]
    fn theme_list_always_offers_the_default() {
        assert!(Theme::get_theme_list().iter().any(|t| t == "default"));
    }

    #[test]
    fn load_from_file_reports_missing_files_instead_of_exiting() {
        let err = Theme::load_from_file(Path::new("/nonexistent/rodeo-theme.toml"))
            .expect_err("missing theme file must be an error");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn unknown_theme_name_falls_back_instead_of_failing() {
        let theme = Theme::load_theme(Some("definitely-not-a-theme"))
            .expect("unknown theme must fall back, not fail");
        assert!(!theme.name.is_empty());
    }

    #[test]
    fn hex_to_color_parses_valid_hex() {
        let color = Color::hex_to_color("#ff5733");
        assert_eq!(color, Color::Rgb(255, 87, 51));
    }

    #[test]
    fn hex_to_color_handles_no_hash() {
        let color = Color::hex_to_color("89b4fa");
        assert_eq!(color, Color::Rgb(137, 180, 250));
    }

    #[test]
    fn deserialize_theme_from_toml() {
        let toml_str = r##"name = "test-theme"
description = "A test theme"

[colors]
background = "#1e1e2e"
foreground = "#cdd6f4"
primary = "#89b4fa"
secondary = "#f38ba8"
accent1 = "#f9e2af"
accent2 = "#a6e3a1"
accent3 = "#b4befe"
muted = "#6c7086"
border = "#45475a"
highlight = "#cba6f7"
surface = "#313244"
error = "#f38ba8"
warning = "#fab387"
info = "#94e2d5"
success = "#a6e3a1"
"##;
        let theme: Theme = toml::from_str(toml_str).unwrap();
        assert_eq!(theme.name, "test-theme");
        assert_eq!(theme.colors.background(), Color::Rgb(30, 30, 46));
        assert_eq!(theme.colors.primary(), Color::Rgb(137, 180, 250));
    }

    // Note: load_theme and get_theme_list tests are skipped because:
    // 1. They read from the filesystem (themes/ directory)
    // 2. load_from_file calls process::exit on error (can't test easily)
    // 3. The themes directory may not exist in test environment
}
