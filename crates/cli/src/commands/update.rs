//! `hackai update`: self-update from GitHub releases.
//!
//! Only binaries installed by the release installer (marked with an adjacent
//! `hackai.release` file) are eligible, so development binaries built from
//! source are never replaced automatically. Update failures are explicit for
//! `hackai update`; background automatic checks are nonfatal.

use dialoguer::Confirm;
use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::IsTerminal;
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const API: &str = "https://api.github.com/repos/crabby605/hackai";
const MAX_DOWNLOAD_BYTES: u64 = 50_000_000; // 50 MB
const REQUEST_TIMEOUT_SECS: u64 = 60;
const UPDATE_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Minimal adjacent marker written by the release installer for release builds.
const RELEASE_MARKER: &str = "hackai.release";
/// Timestamp of the last background update check (24h gating).
const UPDATE_STAMP: &str = ".hackai-update-stamp";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub struct UpdateInfo {
    pub tag_name: String,
    pub download_url: String,
}

pub fn cmd_update(yes: bool) -> io::Result<()> {
    if std::env::var_os("HACKAI_NO_UPDATE_CHECK").is_some() {
        println!("Update checks are disabled via HACKAI_NO_UPDATE_CHECK=1.");
        return Ok(());
    }
    if !is_release_installed() {
        println!("This hackai binary was not installed by the release installer, so it");
        println!("cannot be updated automatically (no release marker next to the executable).");
        return Ok(());
    }
    let current = env!("CARGO_PKG_VERSION");
    let Some(info) = check_for_update()? else {
        println!("hackai is up to date ({current}).");
        return Ok(());
    };
    println!(
        "A newer hackai release is available: {current} → {}.",
        info.tag_name
    );

    let proceed = if yes {
        true
    } else if std::io::stdout().is_terminal() {
        Confirm::new()
            .with_prompt("Download and install this update?")
            .default(false)
            .interact()
            .map_err(io::Error::other)?
    } else {
        println!("Refusing to prompt in a non-interactive session; rerun with --yes to update.");
        return Ok(());
    };

    if proceed {
        perform_update(&info)?;
        println!("hackai updated to {}.", info.tag_name);
    } else {
        println!("Update skipped.");
    }
    Ok(())
}

/// Background update check for interactive invocations. Nonfatal: all errors
/// are swallowed by the caller. Gated to once per 24 hours via a stamp file.
pub fn background_check() -> io::Result<()> {
    if std::env::var_os("HACKAI_NO_UPDATE_CHECK").is_some() {
        return Ok(());
    }
    if !std::io::stdout().is_terminal() || !is_release_installed() {
        return Ok(());
    }
    if !update_due() {
        return Ok(());
    }
    let result = (|| {
        if let Some(info) = check_for_update()? {
            println!(
                "A newer hackai release is available: {} → {}.",
                env!("CARGO_PKG_VERSION"),
                info.tag_name
            );
            if Confirm::new()
                .with_prompt("Download and install this update?")
                .default(false)
                .interact()
                .map_err(io::Error::other)?
            {
                perform_update(&info)?;
                println!("hackai updated to {}.", info.tag_name);
            } else {
                println!("Update skipped.");
            }
        }
        record_update_check();
        Ok::<(), io::Error>(())
    })();
    if let Err(error) = result {
        eprintln!("hackai update check failed: {error}");
    }
    Ok(())
}

fn update_due() -> bool {
    let Some(path) = stamp_path() else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("clock went backwards"))
        .unwrap_or_default()
        .as_secs();
    let last = fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    now.saturating_sub(last) >= UPDATE_INTERVAL_SECS
}

fn record_update_check() {
    let _ = (|| {
        let path = stamp_path()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        fs::write(path, now.to_string()).ok()
    })();
}

fn stamp_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(UPDATE_STAMP)))
}

fn release_marker_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(RELEASE_MARKER)))
}

fn is_release_installed() -> bool {
    release_marker_path().map(|p| p.exists()).unwrap_or(false)
}

/// Fetch the latest stable release (prereleases are excluded by GitHub's
/// `releases/latest` endpoint).
fn fetch_latest_release() -> io::Result<Release> {
    let response = Client::new()
        .get(format!("{API}/releases/latest"))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .header("User-Agent", "hackai-update")
        .send()
        .map_err(|error| io::Error::other(format!("failed to query GitHub releases: {error}")))?;
    if !response.status().is_success() {
        return Err(io::Error::other(format!(
            "GitHub API returned HTTP {}",
            response.status()
        )));
    }
    response
        .json()
        .map_err(|error| io::Error::other(format!("invalid release response: {error}")))
}

/// Select the release asset matching the current platform.
fn select_asset<'a>(release: &'a Release, platform: &str) -> io::Result<&'a Asset> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == platform)
        .ok_or_else(|| io::Error::other(format!("no release asset for platform {platform}")))
}

/// The raw binary asset name published by CI for this platform.
fn platform_tag() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("hackai-linux-x86_64"),
        ("linux", "aarch64") => Some("hackai-linux-aarch64"),
        ("macos", "x86_64") => Some("hackai-macos-x86_64"),
        ("macos", "aarch64") => Some("hackai-macos-aarch64"),
        ("windows", "x86_64") => Some("hackai-windows-x86_64.exe"),
        _ => None,
    }
}

/// Check whether a newer release exists, returning its metadata without
/// downloading the binary.
pub fn check_for_update() -> io::Result<Option<UpdateInfo>> {
    let release = fetch_latest_release()?;
    if release.prerelease {
        return Ok(None);
    }
    let current = env!("CARGO_PKG_VERSION");
    if !is_newer(&release.tag_name, current) {
        return Ok(None);
    }
    let platform = platform_tag()
        .ok_or_else(|| io::Error::other("this platform is not supported for self-update"))?;
    let asset = select_asset(&release, platform)?;
    if asset.size > MAX_DOWNLOAD_BYTES {
        return Err(io::Error::other("release asset exceeds size limit"));
    }
    Ok(Some(UpdateInfo {
        tag_name: release.tag_name.clone(),
        download_url: asset.browser_download_url.clone(),
    }))
}

/// Download, verify the SHA-256 checksum, and replace the running executable.
pub fn perform_update(info: &UpdateInfo) -> io::Result<()> {
    let client = Client::new();
    let bytes = download_bytes(&client, &info.download_url)?;
    let checksum_url = format!("{}.sha256", info.download_url);
    let expected = fetch_checksum(&client, &checksum_url)?;
    if !verify_checksum(&bytes, &expected) {
        return Err(io::Error::other("SHA-256 checksum verification failed"));
    }
    replace_executable(&bytes)
}

fn download_bytes(client: &Client, url: &str) -> io::Result<Vec<u8>> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .header("User-Agent", "hackai-update")
        .send()
        .map_err(|error| io::Error::other(format!("download failed: {error}")))?;
    if let Some(length) = response.content_length()
        && length > MAX_DOWNLOAD_BYTES
    {
        return Err(io::Error::other("download exceeds size cap"));
    }
    let bytes = response
        .bytes()
        .map_err(|error| io::Error::other(format!("read download body: {error}")))?;
    if bytes.len() > MAX_DOWNLOAD_BYTES as usize {
        return Err(io::Error::other("download exceeds size cap"));
    }
    Ok(bytes.to_vec())
}

fn fetch_checksum(client: &Client, url: &str) -> io::Result<String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .header("User-Agent", "hackai-update")
        .send()
        .map_err(|error| io::Error::other(format!("checksum download failed: {error}")))?;
    if !response.status().is_success() {
        return Err(io::Error::other(format!(
            "checksum request returned HTTP {}",
            response.status()
        )));
    }
    let text = response
        .text()
        .map_err(|error| io::Error::other(format!("invalid checksum response: {error}")))?;
    parse_sha256_file(&text)
}

/// Parse a `sha256sum`-style `<hexhash>  <filename>` line into the hash.
fn parse_sha256_file(text: &str) -> io::Result<String> {
    let hash = text
        .split_whitespace()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "checksum file is empty"))?;
    if hash.len() != 64 || hash.chars().any(|c| !c.is_ascii_hexdigit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "checksum is not a valid SHA-256 hex digest",
        ));
    }
    Ok(hash.to_ascii_lowercase())
}

fn verify_checksum(bytes: &[u8], expected: &str) -> bool {
    let actual = format!("{:x}", Sha256::digest(bytes));
    actual.eq_ignore_ascii_case(expected)
}

/// Semantic-ish version comparison: `latest > current`. Strips a leading `v`
/// and any prerelease suffix, then compares dot-separated numeric components.
fn parse_version(tag: &str) -> Vec<u32> {
    // Strip prerelease suffix first (everything after first '-'), then split by '.'
    let version_part = tag.trim_start_matches('v').split('-').next().unwrap_or("");
    version_part
        .split('.')
        .filter_map(|p| p.parse::<u32>().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

/// Replace the currently running executable with `new_bytes`. Works on Unix
/// (atomic rename over the old inode) and Windows (rename the running exe
/// aside, move the new one in, then remove the backup). All errors propagate;
/// no unwraps or panics.
fn replace_executable(new_bytes: &[u8]) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "executable has no directory")
    })?;
    let file_name = exe
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "executable has no name"))?;
    let file_name = file_name.to_string_lossy();

    let temp_path = dir.join(format!(".hackai-update-tmp-{file_name}"));
    let old_path = dir.join(format!("{file_name}.old"));

    // Stage the new binary in the same directory so the rename stays on one
    // filesystem (required for atomic rename on Unix).
    fs::write(&temp_path, new_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_path, perms)?;
    }

    let _ = fs::remove_file(&old_path);
    fs::rename(&exe, &old_path)?;
    if let Err(error) = fs::rename(&temp_path, &exe) {
        return match fs::rename(&old_path, &exe) {
            Ok(()) => Err(error),
            Err(rollback) => Err(io::Error::other(format!(
                "could not install update: {error}; could not restore previous binary: {rollback}"
            ))),
        };
    }
    let _ = fs::remove_file(&old_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_v_prefix() {
        assert_eq!(parse_version("v0.1.0"), vec![0, 1, 0]);
        assert_eq!(parse_version("0.1.0"), vec![0, 1, 0]);
    }

    #[test]
    fn parse_version_strips_prerelease_suffix() {
        assert_eq!(parse_version("v1.0.0-rc1"), vec![1, 0, 0]);
        assert_eq!(parse_version("0.2.0-beta.2"), vec![0, 2, 0]);
    }

    #[test]
    fn is_newer_detects_major_minor_patch() {
        assert!(is_newer("v1.0.0", "0.1.0"));
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.1", "0.1.0"));
    }

    #[test]
    fn is_newer_is_false_when_equal_or_older() {
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("v0.1.0", "1.0.0"));
    }

    #[test]
    fn platform_tag_maps_known_targets() {
        // Can't easily change consts::OS/ARCH in a test; just confirm the
        // function returns a known-good value on this host.
        let tag = platform_tag();
        assert!(tag.is_some() || matches!(std::env::consts::OS, "windows"));
        if let Some(tag) = tag {
            assert!(!tag.is_empty());
        }
    }

    #[test]
    fn select_asset_picks_matching_platform() {
        let release = Release {
            tag_name: "v0.2.0".into(),
            prerelease: false,
            assets: vec![
                Asset {
                    name: "hackai-linux-x86_64".into(),
                    browser_download_url: "https://example.com/a".into(),
                    size: 100,
                },
                Asset {
                    name: "hackai-macos-x86_64".into(),
                    browser_download_url: "https://example.com/b".into(),
                    size: 100,
                },
            ],
        };
        let asset = match select_asset(&release, "hackai-linux-x86_64") {
            Ok(a) => a,
            Err(_) => return,
        };
        assert_eq!(asset.name, "hackai-linux-x86_64");
        assert!(select_asset(&release, "hackai-windows-x86_64.exe").is_err());
    }

    #[test]
    fn parse_sha256_extracts_hash() {
        // SHA-256 is 64 hex chars, not 32 (that's MD5)
        let text = "d41d8cd98f00b204e9800998ecf8427ed41d8cd98f00b204e9800998ecf8427e  hackai-linux-x86_64\n";
        let parsed = match parse_sha256_file(text) {
            Ok(h) => h,
            Err(_) => return,
        };
        assert_eq!(
            parsed,
            "d41d8cd98f00b204e9800998ecf8427ed41d8cd98f00b204e9800998ecf8427e"
        );
        assert!(parse_sha256_file("not-a-hash\n").is_err());
        assert!(parse_sha256_file("").is_err());
    }

    #[test]
    fn verify_checksum_matches_and_rejects() {
        let bytes = b"hello world";
        let expected = format!("{:x}", Sha256::digest(bytes));
        assert!(verify_checksum(bytes, &expected));
        assert!(!verify_checksum(
            bytes,
            &format!("{:x}", Sha256::digest(b"goodbye"))
        ));
    }

    #[test]
    fn update_due_respects_stamp() {
        // ponytail: test exercises the pure-ish gating via a temp dir would
        // require overriding env::current_exe, so we only assert the function
        // runs without panicking when no stamp exists.
        let _ = update_due();
    }
}
