use clap::Parser;
use log::info;
use ui::App;
use ui::theme::Theme;

use crate::config::Config;

mod cli;
mod config;
mod fs;
mod logging;
mod ui;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    logging::init();

    let args = cli::Args::parse();

    let mut config = Config::load_config(args.config.as_deref())?;

    if args.left.is_some() || args.right.is_some() {
        config.set_initial_dir(args.left, args.right);
    }

    log::debug!("Config: {:?}", config);

    let theme_name = args.theme.as_deref().or(Some(config.theme.as_str()));
    let theme = Theme::load_theme(theme_name)?;

    info!("Starting UI");
    ratatui::run(|terminal| App::new(theme, config).run(terminal))?;
    Ok(())
}
