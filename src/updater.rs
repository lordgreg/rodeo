use serde::{Deserialize, Serialize};

use crate::config::CONFIG_DIR;
use std::{
    env,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
};

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
    /// Resolve the cache file path, regardless of whether it exists yet.
    /// Use this when the file is about to be created/overwritten (e.g. `save_info`).
    fn cache_file_path() -> io::Result<PathBuf> {
        xdg::BaseDirectories::with_prefix(CONFIG_DIR)
            .get_cache_file(CACHE_FILE)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidFilename,
                    format!("Cannot read cache file {}", CACHE_FILE),
                )
            })
    }

    /// Resolve the cache file path and require that it already exists.
    /// Use this when reading (e.g. `load_info`, `is_update_pending`).
    fn get_cache_file() -> io::Result<PathBuf> {
        let cache_file_path = Self::cache_file_path()?;

        if Path::new(cache_file_path.as_path()).exists() {
            Ok(cache_file_path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Cache file {:?} doesnt exist.", cache_file_path),
            ))
        }
    }

    fn cleanup_cache_directory() -> io::Result<()> {
        let cache_path = xdg::BaseDirectories::with_prefix(CONFIG_DIR).get_cache_home();

        if let Some(path) = cache_path {
            let cache_path = path.clone();
            log::debug!("cache dir found {:?}", cache_path);

            for entry in std::fs::read_dir(path)? {
                let entry = entry?;
                let entry_path = entry.path();

                if entry_path.is_file() {
                    log::debug!(
                        "Removing {:?} in cache dir {:?}",
                        entry_path.to_str(),
                        cache_path,
                    );

                    std::fs::remove_file(entry_path)?
                }
            }
        }

        Ok(())
    }

    pub fn save_info(&self, cache_file_info: &CacheFileInfo) -> io::Result<()> {
        let cache_file = Self::cache_file_path()?;

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

                let _ = CacheFileInfo::cleanup_cache_directory();

                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Cache file data corrupted.",
                ));
            }
        };

        return Ok(info);
    }

    pub fn asset_shasum_check(asset: &PathBuf, digest: &String) -> Result<bool, io::Error> {
        let sha256_file = Command::new("sha256sum").arg(asset).output()?;

        if sha256_file.status.success() {
            let stdout = String::from_utf8_lossy(&sha256_file.stdout);
            let only_sha = stdout.split_whitespace().next().unwrap();
            let mut cleaned_digest = digest.clone();

            if let Some(stripped) = cleaned_digest.strip_prefix("sha256:") {
                cleaned_digest = stripped.to_string()
            }

            return Ok(only_sha == cleaned_digest.to_string());
        }

        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Incompatible shasum check!"),
        ));
    }

    pub fn package_name(&self) -> String {
        self.asset
            .name
            .strip_suffix(".tar.gz")
            .unwrap_or(&self.asset.name)
            .to_string()
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

            let cache_info = cache_file.ok()?;
            let xdg_dirs = xdg::BaseDirectories::with_prefix(CONFIG_DIR);

            let release_file_archived = xdg_dirs.get_cache_file(&cache_info.asset.name)?;

            // if asset file doesnt exist yet
            if release_file_archived.exists() {
                log::debug!(
                    "Asset file {} already exists. Skipping re-download.",
                    release_file_archived.display()
                )
            } else {
                let client = reqwest::Client::builder()
                    .user_agent(format!("rodeo/{}", env!("CARGO_PKG_VERSION")))
                    .build()
                    .ok()?;

                let response = client
                    .get(&cache_info.asset.browser_download_url)
                    .send()
                    .await
                    .ok()?;

                if !response.status().is_success() {
                    log::debug!(
                        "Updater: could not download asset: {}",
                        response.status().as_str()
                    );
                    return Some(UpdateCheckResult::Failed(format!(
                        "asset download error: {}",
                        response.status().as_str()
                    )));
                }

                let content = response.bytes().await.ok()?;

                log::debug!(
                    "Saving downloaded asset '{}' to {:?}",
                    cache_info.asset.name,
                    release_file_archived
                );

                let mut dest = File::create(&release_file_archived).ok()?;
                dest.write_all(&content).ok()?;
            }

            match CacheFileInfo::asset_shasum_check(
                &release_file_archived,
                &cache_info.asset.digest,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    return Some(UpdateCheckResult::Failed(format!("Shasum check failed!")));
                }
                Err(e) => {
                    return Some(UpdateCheckResult::Failed(format!(
                        "Critical shasum error: {}",
                        e
                    )));
                }
            };

            let file = match File::open(release_file_archived) {
                Ok(f) => f,
                Err(e) => {
                    return Some(UpdateCheckResult::Failed(format!(
                        "Cannot open tar file: {}",
                        e
                    )));
                }
            };

            // create tempdir
            let tmp_dir = match tempfile::tempdir() {
                Ok(dir) => dir,
                Err(e) => {
                    return Some(UpdateCheckResult::Failed(format!(
                        "Failed to create temp directory: {}",
                        e
                    )));
                }
            };

            let gzipped: bool = cache_info.asset.name.ends_with("tar.gz");

            let reader: Box<dyn Read> = if gzipped {
                Box::new(flate2::read::GzDecoder::new(file))
            } else {
                Box::new(file)
            };
            let mut ar = tar::Archive::new(reader);

            if let Err(e) = ar.unpack(&tmp_dir) {
                return Some(UpdateCheckResult::Failed(format!(
                    "Failed unpacking archive file into temp directory: {:?}",
                    e
                )));
            }

            if !Path::new(&xdg_dirs.get_data_home()?.join("themes/")).exists() {
                log::debug!(
                    "Creating XDG_DATA_HOME/rodeo/themes directory {:?}",
                    &xdg_dirs.get_data_home().unwrap().join("themes/")
                );
                match xdg_dirs.create_data_directory(Path::new("themes/")) {
                    Ok(_) => {}
                    Err(e) => {
                        return Some(UpdateCheckResult::Failed(format!(
                            "Cannot create data directory for themes ({:?}): {}",
                            xdg_dirs.get_data_home(),
                            e,
                        )));
                    }
                }
            }

            let bin_current = std::env::current_exe().ok()?;
            let bin_dir = match bin_current.parent() {
                Some(dir) => dir,
                None => {
                    return Some(UpdateCheckResult::Failed(
                        "Current binary parent should be a directory!".to_string(),
                    ));
                }
            };
            let package_name = cache_info.package_name();

            let themes_dir = tmp_dir.path().join(&package_name).join("themes");
            let themes_dest = xdg_dirs.get_data_home()?.join("themes");

            let mut read_dir = match std::fs::read_dir(&themes_dir) {
                Ok(rd) => rd,
                Err(e) => {
                    return Some(UpdateCheckResult::Failed(format!(
                        "Couldn't open themes dir {:?}: {}",
                        themes_dir, e
                    )));
                }
            };

            let result = read_dir.try_for_each(|entry| -> io::Result<()> {
                let entry = entry?;
                std::fs::copy(entry.path(), themes_dest.join(entry.file_name()))?;
                Ok(())
            });

            if let Err(e) = result {
                return Some(UpdateCheckResult::Failed(format!(
                    "Failed copying theme file: {}",
                    e
                )));
            }

            // move binary only when not in debug mode
            if !cfg!(debug_assertions) {
                let bin_new = tmp_dir.path().join(&package_name).join("rodeo");
                let bin_tmp = bin_dir.join(".rodeo.update.tmp");

                if let Err(e) = fs::copy(&bin_new, &bin_tmp) {
                    return Some(UpdateCheckResult::Failed(format!(
                        "Couldnt copy binary: {:?}",
                        e
                    )));
                }

                if let Err(e) = fs::rename(&bin_tmp, &bin_current) {
                    let _ = fs::remove_file(&bin_tmp);
                    return Some(UpdateCheckResult::Failed(format!(
                        "Couldnt overwrite tmp bin with current bin: {}",
                        e
                    )));
                }
            }

            return Some(UpdateCheckResult::Updated(cache_info.version));
        }

        Some(UpdateCheckResult::Incompatible)
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
    }

    pub fn is_update_pending() -> bool {
        if CacheFileInfo::get_cache_file().is_ok() {
            return true;
        }

        false
    }

    pub fn cleanup_everything() {
        if cfg!(debug_assertions) {
            log::debug!("Cleanup everything skipped while in debug mode.");
            return;
        }
        let xdg_dirs = xdg::BaseDirectories::with_prefix(CONFIG_DIR);

        let cache_home = match xdg_dirs.get_cache_home() {
            Some(cache_dir) => cache_dir,
            None => return,
        };

        log::debug!("Removing cache dir {:?}", cache_home);
        let _ = fs::remove_dir_all(cache_home);
    }
}
