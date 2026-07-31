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
    ./themes                      when running from a source checkout

KEYS:
    F1 in the app lists every binding; see also the README.

EXAMPLES:
    rodeo                              open both panes at the configured directories
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
