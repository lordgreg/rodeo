use log::info;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

pub const DEFAULT_THEME_FILENAME: &str = "default.yaml";
pub const DEFAULT_THEME_DIR: &str = "themes";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Colors {
    pub background: String,
    pub foreground: String,
    pub primary: String,
    pub secondary: String,
    pub success: String,
    pub warning: String,
    pub error: String,
    pub info: String,
    pub muted: String,
    pub border: String,
    pub surface: String,
    pub highlight: String,
    pub accent1: String,
    pub accent2: String,
    pub accent3: String,
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

    pub fn accent2(&self) -> Color {
        Color::hex_to_color(&self.accent2)
    }

    pub fn accent3(&self) -> Color {
        Color::hex_to_color(&self.accent3)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub description: String,
    pub colors: Colors,
}

impl Theme {
    pub fn load_theme(name: Option<&str>) -> Self {
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

    pub fn load_from_file(filename: &str) -> Self {
        let theme_str = std::fs::read_to_string(filename).expect("Failed to read theme file");
        let theme: Theme = yaml_serde::from_str(&theme_str).expect("Failed to parse theme file");

        info!("Loaded theme {}", theme.name);
        theme
    }
}
