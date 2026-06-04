use std::io;

use clap::Parser;
use env_logger::Env;
use log::info;
use ui::App;
use ui::theme::Theme;

mod cli;
mod config;
mod ui;

fn main() -> io::Result<()> {
    // env_logger::init();
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let args = cli::Args::parse();

    let config = config::load_config(args.config.as_deref());
    println!("Config: {:?}", config);

    let theme = Theme::load_theme(args.theme.as_deref());

    info!("Starting UI");
    ratatui::run(|terminal| App::new(theme).run(terminal))
}
