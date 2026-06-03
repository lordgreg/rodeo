use clap::Parser;
use env_logger::Env;

mod cli;
mod config;
mod ui;

fn main() -> color_eyre::Result<()> {
    // env_logger::init();
    env_logger::Builder::from_env(Env::default().default_filter_or("warn")).init();

    let args = cli::Args::parse();

    let config = config::load_config(args.config.as_deref());
    println!("Config: {:?}", config);

    ui::run()
}
