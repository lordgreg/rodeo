//! Themes: the colour palette and the syntax-highlighting colours derived from
//! it.
//!
//! Theme files are TOML and are looked up along an XDG search path; a copy of
//! the default theme is compiled in so rodeo always starts.

use std::fmt;
use std::fs;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use log::info;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use syntect::highlighting::{
    Color as SynColor, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem, ThemeSettings,
};

use crate::config::CONFIG_DIR;
use crate::ui::syntax::{PaletteSlot, SYNTAX_RULES};

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

/// Declares the palette once: the struct field, its serde name and its getter
/// all come from this list.
///
/// The field list, the getters, the `Role` enum and the role→colour match used
/// to be four parallel lists kept in step by hand. They had already drifted —
/// `accent2` and `accent3` had no getters, so no widget could reach them even
/// though every bundled theme defines them.
macro_rules! palette {
    ($($name:ident),* $(,)?) => {
        #[derive(Serialize, Deserialize, Debug, Clone)]
        /// The palette. Values are `#rrggbb` (or `#rgb`) strings in the theme
        /// file and are parsed into terminal colours when the theme is loaded.
        pub struct Colors {
            $($name: HexColor,)*
        }

        impl Colors {
            $(
                pub fn $name(&self) -> Color {
                    self.$name.color()
                }
            )*
        }
    };
}

palette!(
    background, foreground, primary, secondary, success, warning, error, info, muted, border,
    surface, highlight, accent1, accent2, accent3,
);

/// A colour written as `#rrggbb` or `#rgb` in a theme file.
///
/// Parsing happens once, during deserialization, so a malformed value is
/// reported with the offending field named and the theme is rejected. Reading a
/// colour afterwards cannot fail, which keeps parsing off the render path — the
/// getters below used to re-parse a string on every access, every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexColor(Color);

impl HexColor {
    pub fn color(self) -> Color {
        self.0
    }
}

/// Parses `#rrggbb`, `#rgb`, or the same without the leading `#`.
///
/// Works on `char`s rather than byte slices: the previous implementation
/// indexed `&hex[0..2]` directly and panicked on any value shorter than six
/// characters (`#fff` is the obvious one) or on a multi-byte character landing
/// across a slice boundary — inside `terminal.draw`, leaving the terminal raw.
fn parse_hex(hex: &str) -> Option<Color> {
    let body = hex.strip_prefix('#').unwrap_or(hex);
    let digits = body
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<Vec<u8>>>()?;

    match digits[..] {
        // Shorthand: each digit is doubled, so `#abc` is `#aabbcc`.
        [r, g, b] => Some(Color::Rgb(r * 0x11, g * 0x11, b * 0x11)),
        [r1, r0, g1, g0, b1, b0] => {
            Some(Color::Rgb(r1 * 0x10 + r0, g1 * 0x10 + g0, b1 * 0x10 + b0))
        }
        _ => None,
    }
}

impl fmt::Display for HexColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Color::Rgb(r, g, b) => write!(f, "#{r:02x}{g:02x}{b:02x}"),
            other => write!(f, "{other}"),
        }
    }
}

impl Serialize for HexColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        parse_hex(&raw).map(HexColor).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid colour '{raw}': expected '#rrggbb' or '#rgb'"
            ))
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
/// A named colour palette, loaded from a TOML file.
pub struct Theme {
    pub name: String,
    pub description: String,
    pub colors: Colors,
}

impl Theme {
    /// Builds a syntect theme from this palette.
    ///
    /// The mapping is a table of (scope selector, palette role, font style)
    /// rather than a formatted XML plist: no runtime parsing, no panic on a
    /// malformed colour, and the rules can be unit tested. Scope names follow
    /// what the bundled Sublime grammars actually emit.
    pub fn to_syntect_theme(&self) -> SynTheme {
        let settings = ThemeSettings {
            foreground: Some(self.syn(Colors::foreground)),
            background: Some(self.syn(Colors::background)),
            ..ThemeSettings::default()
        };

        let scopes = SYNTAX_RULES
            .iter()
            .filter_map(|(selector, slot, font)| {
                let scope = ScopeSelectors::from_str(selector)
                    .map_err(|e| log::warn!("invalid scope selector '{selector}': {e}"))
                    .ok()?;
                Some(ThemeItem {
                    scope,
                    style: StyleModifier {
                        foreground: Some(self.syn(*slot)),
                        background: None,
                        font_style: Some(*font),
                    },
                })
            })
            .collect();

        SynTheme {
            name: Some(self.name.clone()),
            author: Some("rodeo".to_string()),
            settings,
            scopes,
        }
    }

    /// Palette lookup in syntect's colour type.
    fn syn(&self, slot: PaletteSlot) -> SynColor {
        match slot(&self.colors) {
            Color::Rgb(r, g, b) => SynColor { r, g, b, a: 0xFF },
            _ => SynColor {
                r: 0,
                g: 0,
                b: 0,
                a: 0xFF,
            },
        }
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

    /// A theme file with every palette key set to `color`, so a single value
    /// can be put under test without repeating the whole palette.
    fn theme_toml(color: &str) -> String {
        let keys = [
            "background",
            "foreground",
            "primary",
            "secondary",
            "success",
            "warning",
            "error",
            "info",
            "muted",
            "border",
            "surface",
            "highlight",
            "accent1",
            "accent2",
            "accent3",
        ];
        let mut out = String::from("name = \"t\"\ndescription = \"d\"\n[colors]\n");
        for key in keys {
            out.push_str(&format!("{key} = \"{color}\"\n"));
        }
        out
    }

    #[test]
    fn every_palette_colour_has_a_getter() {
        let t = Theme::builtin().unwrap();
        // accent2/accent3 had no getters at all: defined in every bundled
        // theme, reachable from no widget.
        for (name, got) in [
            ("background", t.colors.background()),
            ("foreground", t.colors.foreground()),
            ("primary", t.colors.primary()),
            ("secondary", t.colors.secondary()),
            ("success", t.colors.success()),
            ("warning", t.colors.warning()),
            ("error", t.colors.error()),
            ("info", t.colors.info()),
            ("muted", t.colors.muted()),
            ("border", t.colors.border()),
            ("surface", t.colors.surface()),
            ("highlight", t.colors.highlight()),
            ("accent1", t.colors.accent1()),
            ("accent2", t.colors.accent2()),
            ("accent3", t.colors.accent3()),
        ] {
            assert!(matches!(got, Color::Rgb(_, _, _)), "{name} did not resolve");
        }
    }

    #[test]
    fn parses_six_digit_hex() {
        assert_eq!(parse_hex("#1a2b3c"), Some(Color::Rgb(0x1a, 0x2b, 0x3c)));
        assert_eq!(parse_hex("1a2b3c"), Some(Color::Rgb(0x1a, 0x2b, 0x3c)));
        assert_eq!(parse_hex("#FFFFFF"), Some(Color::Rgb(255, 255, 255)));
    }

    #[test]
    fn parses_three_digit_shorthand() {
        // Each digit is doubled: #abc is #aabbcc.
        assert_eq!(parse_hex("#abc"), Some(Color::Rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_hex("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex("#000"), Some(Color::Rgb(0, 0, 0)));
    }

    #[test]
    fn rejects_malformed_hex_instead_of_panicking() {
        // Every one of these used to panic on a raw byte slice inside
        // terminal.draw, which left the terminal in raw mode.
        for bad in [
            "", "#", "#f", "#ff", "#ffff", "#fffff", "#fffffff", "#gggggg", "#12345z", "café",
            "#ää", "#ééé", "#aaaéa",
        ] {
            assert_eq!(parse_hex(bad), None, "expected {bad:?} to be rejected");
        }
    }

    #[test]
    fn a_theme_with_a_bad_colour_is_rejected_at_load() {
        // Exactly one key is broken, so the error must point at that key.
        let bad = theme_toml("#1a2b3c").replace("highlight = \"#1a2b3c\"", "highlight = \"#zzz\"");

        let err = Theme::from_str(&bad).expect_err("bad colour must be rejected");
        assert_eq!(err.kind(), ErrorKind::InvalidData);

        // The message must name the offending key and quote the bad value, so
        // the theme file can actually be fixed.
        let msg = err.to_string();
        assert!(msg.contains("highlight"), "{msg}");
        assert!(msg.contains("#zzz"), "{msg}");
    }

    #[test]
    fn a_theme_missing_a_colour_is_rejected_at_load() {
        let missing = theme_toml("#1a2b3c").replace("border = \"#1a2b3c\"\n", "");
        let err = Theme::from_str(&missing).expect_err("incomplete palette must be rejected");
        assert!(err.to_string().contains("border"), "{err}");
    }

    #[test]
    fn a_theme_using_shorthand_colours_loads() {
        let theme = Theme::from_str(&theme_toml("#fff")).expect("shorthand must be accepted");
        assert_eq!(theme.colors.background(), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn hex_colours_round_trip_through_serde() {
        let theme = Theme::from_str(&theme_toml("#1a2b3c")).expect("valid theme");
        let text = toml::to_string(&theme).expect("theme must serialize");
        let again = Theme::from_str(&text).expect("re-parse");
        assert_eq!(again.colors.background(), theme.colors.background());
    }

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

    /// Colour syntect resolves for a scope stack, as an `#rrggbb` string.
    fn color_for(theme: &Theme, scope: &str) -> Color {
        use syntect::highlighting::{HighlightState, Highlighter, RangedHighlightIterator};
        use syntect::parsing::ScopeStack;

        let syn = theme.to_syntect_theme();
        let highlighter = Highlighter::new(&syn);
        let stack = ScopeStack::from_str(scope).expect("valid scope");
        let mut state = HighlightState::new(&highlighter, ScopeStack::new());
        let ops = [(
            0usize,
            syntect::parsing::ScopeStackOp::Push(stack.scopes[0]),
        )];
        let text = "x";
        let mut iter = RangedHighlightIterator::new(&mut state, &ops, text, &highlighter);
        let (style, _, _) = iter.next().expect("one region");
        Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b)
    }

    fn test_theme() -> Theme {
        Theme::builtin().expect("built-in theme")
    }

    #[test]
    fn every_scope_selector_parses() {
        // A typo in a selector would otherwise be silently dropped.
        for (selector, _, _) in SYNTAX_RULES {
            assert!(
                ScopeSelectors::from_str(selector).is_ok(),
                "invalid selector: {selector}"
            );
        }
        assert_eq!(
            test_theme().to_syntect_theme().scopes.len(),
            SYNTAX_RULES.len()
        );
    }

    #[test]
    fn keywords_share_one_colour() {
        let theme = test_theme();
        let primary = theme.colors.primary.color();
        // Rust `use`, `pub`, `let`/`fn`/`struct` (all storage.type*), Python `def`.
        for scope in [
            "keyword.other.rust",
            "storage.modifier.rust",
            "storage.type.rust",
            "storage.type.function.rust",
            "storage.type.class.python",
        ] {
            assert_eq!(color_for(&theme, scope), primary, "scope {scope}");
        }
    }

    #[test]
    fn type_names_differ_from_keywords() {
        let theme = test_theme();
        // Regression: these used to fall through to plain foreground because
        // the rules said entity.name.type.struct, which no grammar emits.
        for scope in [
            "entity.name.struct.rust",
            "entity.name.impl.rust",
            "entity.name.class.python",
        ] {
            assert_eq!(
                color_for(&theme, scope),
                theme.colors.accent2.color(),
                "scope {scope}"
            );
        }
    }

    #[test]
    fn functions_and_macros_share_one_colour() {
        let theme = test_theme();
        // Regression: `println!` (support.macro) used to be uncoloured.
        for scope in [
            "entity.name.function.rust",
            "support.macro.rust",
            "variable.function.python",
        ] {
            assert_eq!(
                color_for(&theme, scope),
                theme.colors.info.color(),
                "scope {scope}"
            );
        }
    }

    #[test]
    fn comments_strings_and_numbers_use_their_roles() {
        let theme = test_theme();
        assert_eq!(
            color_for(&theme, "comment.line.double-slash.rust"),
            theme.colors.muted.color()
        );
        assert_eq!(
            color_for(&theme, "string.quoted.double.rust"),
            theme.colors.success.color()
        );
        assert_eq!(
            color_for(&theme, "constant.numeric.integer.decimal.rust"),
            theme.colors.accent1.color()
        );
        assert_eq!(
            color_for(&theme, "variable.parameter.rust"),
            theme.colors.accent3.color()
        );
    }

    #[test]
    fn markup_tags_are_not_operator_coloured() {
        let theme = test_theme();
        // Regression: entity.name.tag was lumped in with keyword.operator.
        assert_eq!(
            color_for(&theme, "entity.name.tag.block.any.html"),
            theme.colors.primary.color()
        );
        assert_eq!(
            color_for(&theme, "entity.other.attribute-name.class.html"),
            theme.colors.accent3.color()
        );
    }

    #[test]
    fn parses_hex_with_and_without_a_leading_hash() {
        assert_eq!(parse_hex("#ff5733"), Some(Color::Rgb(255, 87, 51)));
        assert_eq!(parse_hex("89b4fa"), Some(Color::Rgb(137, 180, 250)));
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
}
