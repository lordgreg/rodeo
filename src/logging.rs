//! Logging setup.
//!
//! Output goes to a file in the XDG state directory, never to stderr: anything
//! written to the terminal would corrupt the TUI.

use std::fs::File;

use env_logger::{Env, Target};

use crate::config::CONFIG_DIR;

const LOG_FILENAME: &str = "rodeo.log";

/// Log to a file instead of stderr, so log output cannot corrupt the TUI.
pub fn init() {
    let log_path =
        match xdg::BaseDirectories::with_prefix(CONFIG_DIR).place_state_file(LOG_FILENAME) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("Cannot determine log file path: {e}. Logging disabled.");
                return;
            }
        };

    let log_file = match File::options().create(true).append(true).open(&log_path) {
        Ok(file) => file,
        Err(e) => {
            eprintln!(
                "Cannot open log file {}: {e}. Logging disabled.",
                log_path.display()
            );
            return;
        }
    };

    env_logger::Builder::from_env(Env::default().default_filter_or("warn"))
        .target(Target::Pipe(Box::new(log_file)))
        .init();
}
