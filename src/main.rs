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

    let mut startup_notice: Option<(String, bool)> = None;

    if Updater::is_update_pending() {
        match Updater::apply_update() {
            Some(rodeo::updater::UpdateCheckResult::Updated(msg)) => {
                log::debug!("Apply update completed");
                eprintln!("rodeo: updated successfully.");
                Updater::cleanup_everything();

                startup_notice = Some((format!("Update ({}) applied 🤠", msg), false));
            }
            Some(result) => {
                log::debug!("Apply update yielded:\n {:?}", result);

                startup_notice = Some((
                    "Update failed (~/.local/state/rodeo/rodeo.log)".to_string(),
                    true,
                ));
            }
            None => {}
        }
    } else {
        thread::spawn(move || {
            Updater::update_check(config.auto_update)
                .map(|out| log::debug!("Update check completed:\n{:?}", out));
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
    ratatui::run(|terminal| App::new(theme, config, &config_path, startup_notice).run(terminal))?;
    Ok(())
}
