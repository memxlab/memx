use anyhow::{bail, Context, Result};
#[cfg(not(windows))]
use flate2::read::GzDecoder;
use reqwest::header::{ACCEPT, USER_AGENT};
use semver::Version;
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
#[cfg(not(windows))]
use tar::Archive;
use uuid::Uuid;
use zip::ZipArchive;

use crate::os_service;

const DEFAULT_REPO: &str = "memxlab/memx";

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum UpdateOutcome {
    UpToDate {
        current: Version,
    },
    Updated {
        previous: Version,
        latest: Version,
        restarted_service: bool,
    },
    Scheduled {
        previous: Version,
        latest: Version,
        restarted_service: bool,
    },
}

#[derive(Debug, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone)]
struct ResolvedRelease {
    version: Version,
    asset_name: String,
    asset_url: String,
}

pub async fn update_current_binary(
    binary_path: &Path,
    current_version: &Version,
) -> Result<UpdateOutcome> {
    let release = fetch_latest_release(current_version).await?;

    if release.version <= *current_version {
        return Ok(UpdateOutcome::UpToDate {
            current: current_version.clone(),
        });
    }

    let temp_dir = unique_temp_dir("memx-update");
    fs::create_dir_all(&temp_dir)
        .with_context(|| format!("Failed to create {}", temp_dir.display()))?;

    let archive_path = temp_dir.join(&release.asset_name);
    download_archive(&release.asset_url, &archive_path, current_version).await?;

    let extract_dir = temp_dir.join("extract");
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("Failed to create {}", extract_dir.display()))?;

    let extracted_binary = extract_binary(&archive_path, &extract_dir)?;
    verify_binary(&extracted_binary)?;

    let restart_service = os_service::is_installed().unwrap_or(false);
    let mut service_was_stopped = false;
    if restart_service && os_service::stop().is_ok() {
        service_was_stopped = true;
    }

    let result = replace_binary(binary_path, &extracted_binary, restart_service);
    if result.is_err() && service_was_stopped {
        let _ = os_service::start();
    }

    let outcome = result?;

    #[cfg(not(windows))]
    {
        let _ = fs::remove_dir_all(&temp_dir);
    }

    Ok(match outcome {
        #[cfg(not(windows))]
        ReplaceOutcome::Replaced => {
            if restart_service {
                os_service::start()
                    .context("Updated binary but failed to restart background service")?;
            }

            UpdateOutcome::Updated {
                previous: current_version.clone(),
                latest: release.version,
                restarted_service: restart_service,
            }
        }
        #[cfg(windows)]
        ReplaceOutcome::Scheduled => UpdateOutcome::Scheduled {
            previous: current_version.clone(),
            latest: release.version,
            restarted_service: restart_service,
        },
    })
}

pub fn current_version() -> Result<Version> {
    Version::parse(env!("CARGO_PKG_VERSION")).context("Invalid package version")
}

pub fn asset_name_for_current_target() -> Result<&'static str> {
    asset_name_for(env::consts::OS, env::consts::ARCH)
}

async fn fetch_latest_release(current_version: &Version) -> Result<ResolvedRelease> {
    let client = reqwest::Client::new();
    let url = release_api_url();
    let response = client
        .get(&url)
        .header(USER_AGENT, format!("memx/{current_version}"))
        .header(ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .with_context(|| format!("Failed to fetch release metadata from {url}"))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("No stable MemX release is published yet.");
    }

    let response = response
        .error_for_status()
        .with_context(|| format!("Failed to fetch release metadata from {url}"))?;
    let release: ReleaseResponse = response.json().await.context("Invalid release metadata")?;

    let asset_name = asset_name_for_current_target()?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| {
            format!(
                "Release {} does not include asset {}",
                release.tag_name, asset_name
            )
        })?;

    Ok(ResolvedRelease {
        version: normalize_tag(&release.tag_name)?,
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
    })
}

fn release_api_url() -> String {
    if let Ok(url) = env::var("MEMX_RELEASES_API_URL") {
        if !url.trim().is_empty() {
            return url;
        }
    }

    let repo = env::var("MEMX_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string());
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

async fn download_archive(url: &str, destination: &Path, current_version: &Version) -> Result<()> {
    let client = reqwest::Client::new();
    let bytes = client
        .get(url)
        .header(USER_AGENT, format!("memx/{current_version}"))
        .send()
        .await
        .with_context(|| format!("Failed to download update archive from {url}"))?
        .error_for_status()
        .with_context(|| format!("Failed to download update archive from {url}"))?
        .bytes()
        .await
        .context("Failed to read update archive")?;

    fs::write(destination, &bytes)
        .with_context(|| format!("Failed to write {}", destination.display()))?;
    Ok(())
}

fn extract_binary(archive_path: &Path, extract_dir: &Path) -> Result<PathBuf> {
    if archive_path.extension().and_then(|value| value.to_str()) == Some("zip") {
        extract_zip(archive_path, extract_dir)?;
    } else {
        #[cfg(windows)]
        bail!("Unexpected non-zip update archive on Windows");

        #[cfg(not(windows))]
        extract_tar_gz(archive_path, extract_dir)?;
    }

    find_binary(extract_dir).with_context(|| {
        format!(
            "Could not find extracted MemX binary in {}",
            extract_dir.display()
        )
    })
}

#[cfg(not(windows))]
fn extract_tar_gz(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(extract_dir)
        .with_context(|| format!("Failed to unpack {}", archive_path.display()))?;
    Ok(())
}

fn extract_zip(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("Failed to open {}", archive_path.display()))?;
    let mut archive = ZipArchive::new(file).context("Failed to read zip archive")?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .context("Failed to read zip entry")?;
        let output_path = extract_dir.join(entry.mangled_name());

        if entry.is_dir() {
            fs::create_dir_all(&output_path)
                .with_context(|| format!("Failed to create {}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let mut output = fs::File::create(&output_path)
            .with_context(|| format!("Failed to create {}", output_path.display()))?;
        std::io::copy(&mut entry, &mut output)
            .with_context(|| format!("Failed to extract {}", output_path.display()))?;
    }

    Ok(())
}

fn find_binary(directory: &Path) -> Result<PathBuf> {
    let expected_name = if cfg!(windows) { "memx.exe" } else { "memx" };

    for entry in fs::read_dir(directory)
        .with_context(|| format!("Failed to read {}", directory.display()))?
    {
        let entry = entry.with_context(|| format!("Failed to read {}", directory.display()))?;
        let path = entry.path();

        if path.is_dir() {
            if let Ok(found) = find_binary(&path) {
                return Ok(found);
            }
            continue;
        }

        if path.file_name().and_then(|value| value.to_str()) == Some(expected_name) {
            return Ok(path);
        }
    }

    bail!("MemX binary not found")
}

fn verify_binary(binary_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(binary_path)
            .with_context(|| format!("Failed to stat {}", binary_path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(binary_path, permissions)
            .with_context(|| format!("Failed to set permissions on {}", binary_path.display()))?;
    }

    let output = Command::new(binary_path)
        .arg("--help")
        .output()
        .with_context(|| format!("Failed to run {}", binary_path.display()))?;

    if !output.status.success() {
        bail!("Downloaded binary failed verification");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ReplaceOutcome {
    #[cfg(not(windows))]
    Replaced,
    #[cfg(windows)]
    Scheduled,
}

fn replace_binary(
    current_binary: &Path,
    new_binary: &Path,
    restart_service: bool,
) -> Result<ReplaceOutcome> {
    #[cfg(windows)]
    {
        schedule_windows_replace(current_binary, new_binary, restart_service)?;
        Ok(ReplaceOutcome::Scheduled)
    }

    #[cfg(not(windows))]
    {
        let _ = restart_service;
        replace_binary_unix(current_binary, new_binary)?;
        Ok(ReplaceOutcome::Replaced)
    }
}

#[cfg(not(windows))]
fn replace_binary_unix(current_binary: &Path, new_binary: &Path) -> Result<()> {
    let backup_path = backup_path(current_binary);
    let staging_path = current_binary.with_extension("new");

    if backup_path.exists() {
        fs::remove_file(&backup_path)
            .with_context(|| format!("Failed to remove {}", backup_path.display()))?;
    }
    if staging_path.exists() {
        fs::remove_file(&staging_path)
            .with_context(|| format!("Failed to remove {}", staging_path.display()))?;
    }

    fs::copy(new_binary, &staging_path)
        .with_context(|| format!("Failed to stage update at {}", staging_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = fs::Permissions::from_mode(0o755);
        fs::set_permissions(&staging_path, permissions)
            .with_context(|| format!("Failed to set permissions on {}", staging_path.display()))?;
    }

    if current_binary.exists() {
        fs::rename(current_binary, &backup_path)
            .with_context(|| format!("Failed to back up {}", current_binary.display()))?;
    }

    if let Err(err) = fs::rename(&staging_path, current_binary) {
        if backup_path.exists() {
            let _ = fs::rename(&backup_path, current_binary);
        }
        return Err(err).with_context(|| format!("Failed to replace {}", current_binary.display()));
    }

    Ok(())
}

#[cfg(windows)]
fn schedule_windows_replace(
    current_binary: &Path,
    new_binary: &Path,
    restart_service: bool,
) -> Result<()> {
    let backup_path = backup_path(current_binary);
    let script_path = unique_temp_dir("memx-update-script").with_extension("cmd");
    let restart_line = if restart_service {
        "schtasks /Run /TN MemX >NUL 2>&1".to_string()
    } else {
        String::new()
    };

    let script = format!(
        "@echo off\r\nping 127.0.0.1 -n 3 >NUL\r\nif exist \"{backup}\" del /f /q \"{backup}\" >NUL 2>&1\r\nif exist \"{current}\" move /y \"{current}\" \"{backup}\" >NUL\r\nmove /y \"{new}\" \"{current}\" >NUL\r\n{restart}\r\ndel /f /q \"%~f0\" >NUL 2>&1\r\n",
        backup = backup_path.display(),
        current = current_binary.display(),
        new = new_binary.display(),
        restart = restart_line,
    );

    fs::write(&script_path, script)
        .with_context(|| format!("Failed to write {}", script_path.display()))?;

    Command::new("cmd")
        .args(["/C", &script_path.to_string_lossy()])
        .spawn()
        .with_context(|| format!("Failed to schedule update with {}", script_path.display()))?;

    Ok(())
}

fn backup_path(binary_path: &Path) -> PathBuf {
    let file_name = binary_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("memx");
    binary_path.with_file_name(format!("{file_name}.bak"))
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()))
}

fn normalize_tag(tag_name: &str) -> Result<Version> {
    Version::parse(tag_name.trim_start_matches('v'))
        .with_context(|| format!("Invalid release tag: {tag_name}"))
}

fn asset_name_for(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("macos", "x86_64") | ("macos", "aarch64") => Ok("memx-darwin-universal.tar.gz"),
        ("linux", "x86_64") => Ok("memx-linux-x86_64.tar.gz"),
        ("windows", "x86_64") => Ok("memx-windows-x86_64.zip"),
        ("linux", "aarch64") => bail!("Linux aarch64 updates are not published yet."),
        ("windows", "aarch64") => bail!("Windows arm64 updates are not published yet."),
        _ => bail!("Unsupported target for update: {os}/{arch}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{asset_name_for, normalize_tag};

    #[test]
    fn normalize_tag_supports_v_prefix() {
        let version = normalize_tag("v1.2.3").unwrap();
        assert_eq!(version.to_string(), "1.2.3");
    }

    #[test]
    fn normalize_tag_supports_plain_semver() {
        let version = normalize_tag("2.0.0-rc.1").unwrap();
        assert_eq!(version.to_string(), "2.0.0-rc.1");
    }

    #[test]
    fn asset_name_for_known_targets() {
        assert_eq!(
            asset_name_for("macos", "aarch64").unwrap(),
            "memx-darwin-universal.tar.gz"
        );
        assert_eq!(
            asset_name_for("linux", "x86_64").unwrap(),
            "memx-linux-x86_64.tar.gz"
        );
        assert_eq!(
            asset_name_for("windows", "x86_64").unwrap(),
            "memx-windows-x86_64.zip"
        );
    }
}
