use crate::config::Config;
use std::{env, process::Command};

///! Auto updater

pub enum UpdateCheckResult {
    NoUpdate,
    Updated(String),
    Failed(String),
    Disabled,
    Incompatible,
}

const BREW_FORMULA_NAME: &str = "lordgreg/rodeo";
const GIT_RELEASE_API: &str = "https://api.github.com/repos/lordgreg/rodeo/releases/latest";

pub struct OsInfo {}

impl OsInfo {
    pub fn is_linux() -> bool {
        env::consts::OS == "linux" && env::consts::ARCH == "x86_64"
    }

    pub fn is_mac() -> bool {
        env::consts::OS == "macos" && env::consts::ARCH == "arm"
    }
}

pub struct Updater {}

impl Updater {
    #[tokio::main]
    pub async fn update_check(config: &Config) -> Option<UpdateCheckResult> {
        if !config.auto_update {
            return Some(UpdateCheckResult::Disabled);
        };

        // 1) macos
        if OsInfo::is_mac() {
            let output = Command::new("brew")
                .args(["outdated", BREW_FORMULA_NAME])
                .output()
                .ok()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::debug!("Updater: brew execution failed: {:?}", stderr);
                return Some(UpdateCheckResult::Failed(format!(
                    "brew update failed: {}",
                    stderr,
                )));
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.contains("out of date") {
                log::debug!("Updater: No new version available.");
                return Some(UpdateCheckResult::NoUpdate);
            }

            // macos update
            let update = Command::new("brew")
                .args(["upgrade", BREW_FORMULA_NAME])
                .output()
                .ok()?;

            if update.status.success() {
                return Some(UpdateCheckResult::Updated(format!(
                    "brew upgrade {} executed successfully.",
                    BREW_FORMULA_NAME
                )));
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log::debug!("Updater: brew update failed: {}", stderr);
                return Some(UpdateCheckResult::Failed(format!(
                    "brew upgrade failed: {}",
                    stderr
                )));
            }
        } else if OsInfo::is_linux() {
            let current = env!("CARGO_PKG_VERSION");

            // find latest release
            let response = reqwest::get(GIT_RELEASE_API).await.ok()?;

            if !response.status().is_success() {
                log::debug!(
                    "Updater: could not get github api response: {}",
                    response.status().as_str()
                );
                return Some(UpdateCheckResult::Failed(format!(
                    "github api download error: {}",
                    response.status().as_str()
                )));
            }

            let json: serde_json::Value = response.json().await.ok()?;

            let filename = json["assets"].as_array().and_then(|assets| {
                assets.iter().find(|asset| {
                    asset["name"]
                        .as_str()
                        .map(|name| {
                            let name = name.to_lowercase();

                            name.contains("x64") && name.contains("linux")
                        })
                        .unwrap_or(false)
                })
            });

            if let Some(filename) = filename {
                // downloading tar.gz file

                return Some(UpdateCheckResult::Updated(format!(
                    "trying to download file {}",
                    filename
                )));
            } else {
                return Some(UpdateCheckResult::Failed(format!(
                    "failed finding asset name from github api {} (containing x64 and linux)",
                    GIT_RELEASE_API
                )));
            }

            // TODO: Fix this
            return Some(UpdateCheckResult::NoUpdate);
        } else {
            log::debug!(
                "Updater: Incompatible os {} {}",
                env::consts::OS,
                env::consts::ARCH
            );
        }

        return Some(UpdateCheckResult::Incompatible);

        // check version

        // notify
    }

    pub fn apply_update() {}
}
