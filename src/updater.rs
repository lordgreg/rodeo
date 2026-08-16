use serde::{Deserialize, Serialize};

use crate::config::CONFIG_DIR;
use std::{env, io, path::PathBuf, process::Command};

///! Auto updater

const CACHE_FILE: &str = "update";
const BREW_FORMULA_NAME: &str = "lordgreg/rodeo";
const GIT_RELEASE_API: &str = "https://api.github.com/repos/lordgreg/rodeo/releases/latest";

#[derive(Serialize, Deserialize, Debug)]
pub enum UpdateCheckResult {
    NoUpdate,
    Available(String, String),
    Updated(String),
    Failed(String),
    Disabled,
    Incompatible,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CacheFileInfo {
    version: String,
    asset: GithubAsset,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GithubAsset {
    name: String,
    browser_download_url: String,
    content_type: String,
    digest: String,
}

impl CacheFileInfo {
    fn get_cache_file() -> io::Result<PathBuf> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix(CONFIG_DIR);

        xdg_dirs.get_cache_file(CACHE_FILE).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidFilename,
                format!("Cannot read cache file {}", CACHE_FILE),
            )
        })
    }

    pub fn save_info(&self, cache_file_info: &CacheFileInfo) -> io::Result<()> {
        let cache_file = Self::get_cache_file()?;

        let file_info_to_str = serde_json::to_string(&cache_file_info).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Failed do serialize json content: {}", e),
            )
        })?;

        let parent_dir = cache_file.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cache path has no parent directory.",
            )
        })?;

        std::fs::create_dir_all(parent_dir)?;

        log::debug!(
            "Saving {} to cache file {:?}",
            file_info_to_str,
            cache_file.to_str()
        );

        std::fs::write(cache_file, file_info_to_str)
    }

    pub fn load_info() -> io::Result<CacheFileInfo> {
        let cache_file = Self::get_cache_file()?;
        let cache_file_info_str = std::fs::read_to_string(cache_file)?;

        let info = match serde_json::from_str(&cache_file_info_str) {
            Ok(data) => data,
            Err(err) => {
                log::debug!("Updater: Cannot fetch data from cache file: {}", err);

                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Cache file data corrupted.",
                ));
            }
        };

        return Ok(info);
    }
}

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
    pub async fn apply_update() -> Option<UpdateCheckResult> {
        if OsInfo::is_mac() {
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
            }

            let stderr = String::from_utf8_lossy(&update.stderr);
            log::debug!("Updater: brew update failed: {}", stderr);
            return Some(UpdateCheckResult::Failed(format!(
                "brew upgrade failed: {}",
                stderr
            )));
        } else if OsInfo::is_linux() {
            let cache_file = CacheFileInfo::load_info();

            if cache_file.is_err() {
                return Some(UpdateCheckResult::Failed(format!(
                    "Failed to get cache file"
                )));
            }

            // let cache_info = cache_file.ok()?;

            // const DOWNLOAD_URL =

            // read cache file info regarding new version
            // let cache_file

            // xdg::BaseDirectories::get_cache_file(&self, "test")

            // if let Some(filename) = filename {
            //     // downloading tar.gz file

            //     return Some(UpdateCheckResult::Updated(format!(
            //         "trying to download file {}",
            //         filename
            //     )));
            // } else {
            //     return Some(UpdateCheckResult::Failed(format!(
            //         "failed finding asset name from github api {} (containing x64 and linux)",
            //         GIT_RELEASE_API
            //     )));
            // }

            // TODO: Fix this
            return Some(UpdateCheckResult::NoUpdate);
        }

        Some(UpdateCheckResult::Updated(String::from("ok")))
    }

    #[tokio::main]
    pub async fn update_check(allow_to_update: bool) -> Option<UpdateCheckResult> {
        if !allow_to_update {
            return Some(UpdateCheckResult::Disabled);
        };

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

            let cache_file_info = CacheFileInfo {
                version: String::from("aaa"),
                asset: GithubAsset {
                    name: env!("CARGO_PKG_NAME").to_string(),
                    browser_download_url: env!("CARGO_PKG_REPOSITORY").to_string(),
                    content_type: String::from("n/a"),
                    digest: String::from("n/a"),
                },
            };

            return match cache_file_info.save_info(&cache_file_info) {
                Ok(_) => Some(UpdateCheckResult::Available(
                    cache_file_info.version,
                    cache_file_info.asset.browser_download_url,
                )),
                Err(err) => Some(UpdateCheckResult::Failed(format!("err {}", err))),
            };
        } else if OsInfo::is_linux() {
            let current = env!("CARGO_PKG_VERSION");

            // find latest release
            // GitHub's API rejects requests without a User-Agent header with a 403.
            let client = reqwest::Client::builder()
                .user_agent(format!("rodeo/{}", env!("CARGO_PKG_VERSION")))
                .build()
                .ok()?;
            let response = client.get(GIT_RELEASE_API).send().await.ok()?;

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

            let github_release: GithubRelease = response.json().await.ok()?;

            let mut latest = github_release.tag_name;

            if latest.starts_with("v") {
                latest.remove(0);
            }

            if current.eq(&latest) {
                return Some(UpdateCheckResult::NoUpdate);
            }

            let asset = github_release
                .assets
                .iter()
                .find(|asset| asset.name.contains("x86_64") && asset.name.contains("linux"));

            if asset.is_none() {
                return Some(UpdateCheckResult::Failed(format!(
                    "Last version not found using github api."
                )));
            }

            let cache_file_info = CacheFileInfo {
                version: latest,
                asset: asset.expect("Asset is expected, but not found.").clone(),
            };

            return match cache_file_info.save_info(&cache_file_info) {
                Ok(_) => Some(UpdateCheckResult::Available(
                    cache_file_info.version,
                    cache_file_info.asset.browser_download_url,
                )),
                Err(err) => Some(UpdateCheckResult::Failed(format!("err {}", err))),
            };
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

    pub fn is_update_pending() -> bool {
        if CacheFileInfo::get_cache_file().is_ok() {
            log::debug!("Found cache file, seems we are in upgrade process..");
            return true;
        }

        false
    }
}
