//! Command-line arguments.
//!
//! Kept deliberately small: rodeo is configured through `config.toml`, and the
//! flags exist to override the parts of it that are worth changing per run.

use clap::Parser;

/// Parsed command-line arguments.
#[derive(Parser, Debug)]
#[command(
    version,
    about = "A dual-pane terminal file manager with Vim-style keybindings",
    long_about = None,
    after_help = "\
CONFIGURATION:
    ~/.config/rodeo/config.toml   created with defaults on first run

THEMES, first match wins:
    $XDG_DATA_HOME/rodeo/themes   e.g. ~/.local/share/rodeo/themes
    $XDG_DATA_DIRS/rodeo/themes   e.g. /usr/share/rodeo/themes
    <bin>/../share/rodeo/themes   the bin/share layout a package installs into
    <bin>/themes                  running out of an extracted release archive
    ./themes                      when running from a source checkout

KEYS:
    ? in the app lists every binding; see also the README.

EXAMPLES:
    rodeo                              open both panes at the configured directories
    rodeo .                            open both panes at the current directory
    rodeo -l ~/src -r /tmp             open the two panes somewhere specific
    rodeo --theme nord                 override the configured theme for this run
    rodeo --config ./rodeo.toml        use a different configuration file"
)]
pub struct Args {
    /// Configuration file to use instead of the default
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<String>,

    /// Theme name, or a path to a theme file ending in .toml
    #[arg(short, long, value_name = "NAME|FILE")]
    pub theme: Option<String>,

    /// Directory for the left pane (overrides the configured one)
    #[arg(short, long, value_name = "PATH")]
    pub left: Option<String>,

    /// Directory for the right pane (overrides the configured one)
    #[arg(short, long, value_name = "PATH")]
    pub right: Option<String>,

    /// Open both panes at this directory (mutually exclusive with --left/--right)
    #[arg(value_name = "PATH", conflicts_with_all = ["left", "right"])]
    pub path: Option<String>,
}

/// Validates that the positional `path` argument exists and is a directory.
///
/// Kept as a pure function so the two distinct error messages and the
/// exists/is-a-directory branches are covered by tests — `main.rs` stays a
/// thin shell that only formats and exits on `Err`.
pub fn validate_path(path: &str) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if !metadata.is_dir() => Err("not a directory".to_string()),
        Ok(_) => Ok(()),
        Err(_) => Err("no such file or directory".to_string()),
    }
}

/// Renders the man page for [`Args`] as roff.
///
/// Lives here rather than in a build script so the page is a checked-in file
/// that packagers can install, with a test guarding it against drift.
pub fn man_page() -> std::io::Result<Vec<u8>> {
    use clap::CommandFactory;

    let mut buffer = Vec::new();
    clap_mangen::Man::new(Args::command()).render(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn a_lone_path_argument_is_parsed_without_left_or_right() {
        let args = Args::try_parse_from(["rodeo", "."]).unwrap();

        assert_eq!(args.path, Some(".".to_string()));
        assert_eq!(args.left, None);
        assert_eq!(args.right, None);
    }

    #[test]
    fn a_path_argument_conflicts_with_left() {
        let result = Args::try_parse_from(["rodeo", ".", "-l", "foo"]);

        let err = result.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn a_path_argument_conflicts_with_right() {
        let result = Args::try_parse_from(["rodeo", ".", "-r", "foo"]);

        let err = result.unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn no_arguments_still_parses_with_no_path() {
        let args = Args::try_parse_from(["rodeo"]).unwrap();

        assert_eq!(args.path, None);
    }

    #[test]
    fn validate_path_rejects_a_nonexistent_path() {
        let result = validate_path("/definitely/not/a/real/path");

        assert_eq!(result, Err("no such file or directory".to_string()));
    }

    #[test]
    fn validate_path_rejects_an_existing_file() {
        let file = tempfile::NamedTempFile::new().unwrap();

        let result = validate_path(file.path().to_str().unwrap());

        assert_eq!(result, Err("not a directory".to_string()));
    }

    #[test]
    fn validate_path_accepts_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();

        let result = validate_path(dir.path().to_str().unwrap());

        assert_eq!(result, Ok(()));
    }
}
