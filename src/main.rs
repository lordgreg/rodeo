use clap::Parser;
use log::info;
use rodeo::config::Config;
use rodeo::ui::App;
use rodeo::ui::theme::Theme;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    rodeo::logging::init();

    let args = rodeo::cli::Args::parse();

    // Held on to: bookmarks are stored beside whichever config was actually
    // read, so `--config ./rodeo.toml` keeps its own set.
    let config_path = Config::get_config_path(args.config.as_deref());
    let mut config = Config::load_config(args.config.as_deref())?;

    if args.left.is_some() || args.right.is_some() {
        config.set_initial_dir(args.left, args.right);
    }

    log::debug!("Config: {:?}", config);

    let theme_name = args.theme.as_deref().or(Some(config.theme.as_str()));
    // A malformed theme must not stop rodeo from starting. The TUI is not up
    // yet, so stderr is safe to write to (see logging::init — the log file
    // alone would be invisible here).
    let theme = match Theme::load_theme(theme_name) {
        Ok(theme) => theme,
        Err(e) => {
            log::warn!("{e}; falling back to the built-in theme");
            eprintln!("rodeo: {e}");
            eprintln!("rodeo: falling back to the built-in theme");
            Theme::builtin()?
        }
    };

    info!("Starting UI");
    ratatui::run(|terminal| App::new(theme, config, &config_path).run(terminal))?;
    Ok(())
}
