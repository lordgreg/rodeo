use std::fs;
use std::io;
use std::io::Cursor;
use std::io::Error;
use std::io::ErrorKind;

use log::info;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use syntect::highlighting::Theme as SynTheme;
use syntect::highlighting::ThemeSet;

pub const DEFAULT_THEME_FILENAME: &str = "default.yaml";
pub const DEFAULT_THEME_DIR: &str = "themes";

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

    pub fn load_theme(name: Option<&str>) -> io::Result<Self> {
        // 1. if name without yaml, we know its in themes directory,
        // 2. if name with yaml, we know its a path to a file
        // 3. if name is None, we load the default theme from themes directory
        let filename;

        if let Some(name) = name {
            if !name.ends_with(".yaml") {
                filename = format!("{}/{}.yaml", DEFAULT_THEME_DIR, name);
            } else {
                filename = name.to_string();
            }
        } else {
            filename = format!("{}/{}", DEFAULT_THEME_DIR, DEFAULT_THEME_FILENAME);
        }
        Self::load_from_file(&filename)
    }

    pub fn get_theme_list() -> Vec<String> {
        let mut themes: Vec<String> = Vec::new();
        let entries = match fs::read_dir(DEFAULT_THEME_DIR) {
            Ok(rd) => rd,
            Err(e) => {
                log::warn!("cannot read theme directory {}: {}", DEFAULT_THEME_DIR, e);
                return themes;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();

            if path.extension().is_none_or(|ext| ext != "yaml") {
                continue;
            }

            if let Some(stem) = path.file_stem()
                && let Some(name) = stem.to_str()
            {
                themes.push(name.to_string());
            }
        }

        themes
    }

    pub fn load_from_file(filename: &str) -> io::Result<Self> {
        let theme_str = match std::fs::read_to_string(filename) {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to read theme file '{}': {}", filename, e);
                log::error!("Available themes:\n{}", Self::get_theme_list().join("\n"));
                std::process::exit(1);
            }
        };

        let theme: Theme = yaml_serde::from_str(&theme_str).map_err(|_| {
            Error::new(
                ErrorKind::InvalidData,
                "cannot parse theme data.".to_string(),
            )
        })?;

        info!("Loaded theme {}", theme.name);
        Ok(theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn deserialize_theme_from_yaml() {
        let yaml = r"name: test-theme
description: A test theme
colors:
  background: '#1e1e2e'
  foreground: '#cdd6f4'
  primary: '#89b4fa'
  secondary: '#f38ba8'
  accent1: '#f9e2af'
  accent2: '#a6e3a1'
  accent3: '#b4befe'
  muted: '#6c7086'
  border: '#45475a'
  highlight: '#cba6f7'
  surface: '#313244'
  error: '#f38ba8'
  warning: '#fab387'
  info: '#94e2d5'
  success: '#a6e3a1'
";
        let theme: Theme = yaml_serde::from_str(yaml).unwrap();
        assert_eq!(theme.name, "test-theme");
        assert_eq!(theme.colors.background(), Color::Rgb(30, 30, 46));
        assert_eq!(theme.colors.primary(), Color::Rgb(137, 180, 250));
    }

    // Note: load_theme and get_theme_list tests are skipped because:
    // 1. They read from the filesystem (themes/ directory)
    // 2. load_from_file calls process::exit on error (can't test easily)
    // 3. The themes directory may not exist in test environment
}
