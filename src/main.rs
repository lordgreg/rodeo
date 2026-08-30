use std::sync::mpsc;
use std::thread;

use clap::Parser;
use log::info;
use rodeo::config::Config;
use rodeo::ui::App;
use rodeo::ui::theme::Theme;
use rodeo::updater::Updater;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    rodeo::logging::init();

    let args = rodeo::cli::Args::parse();

    // Resolved once and held on to. Everything that later reads or writes the
    // configuration — `:w`, `:so` — uses this path, so a session started with
    // `--config` stays with that file; bookmarks are stored beside it too.
    let config_path = Config::get_config_path(args.config.as_deref());
    let mut config = Config::load_config_at(&config_path)?;

    if args.left.is_some() || args.right.is_some() {
        config.set_initial_dir(args.left, args.right);
    }

    log::debug!("Config: {:?}", config);

    // Event emitter which will listen on changes after we already start the
    // TUI. No need to block and await for Updater to finish.
    let (update_notice_tx, update_notice_rx) = mpsc::channel::<(String, bool)>();

    if Updater::is_update_pending() {
        Updater::apply_update_and_notify(&update_notice_tx);
    } else {
        thread::spawn(move || match Updater::update_check(config.auto_update) {
            Some(rodeo::updater::UpdateCheckResult::Available(version, _)) => {
                log::debug!("Update check completed: v{version} available");
                Updater::apply_update_and_notify(&update_notice_tx);
            }
            Some(result) => log::debug!("Update check completed:\n{:?}", result),
            None => {}
        });
    }

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
    ratatui::run(|terminal| {
        let mut app = App::new(theme, config, &config_path);
        app.set_update_notice_rx(update_notice_rx);
        app.run(terminal)
    })?;
    Ok(())
}
