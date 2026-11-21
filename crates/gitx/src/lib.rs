#![forbid(unsafe_code)]
//! Git operations using CLI for maximum compatibility and simplicity.
//! Uses `git` command-line tool for diff extraction and hunk hashing.

use hackai_schema::{GitSha, SaltedHmacHunkHash};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git command failed: {0}")]
    Command(String),
    #[error("repository has no HEAD commit")]
    NoHead,
    #[error("invalid commit id")]
    InvalidId,
    #[error("HMAC error: {0}")]
    Hmac(String),
    #[error("encoding error: {0}")]
    Encoding(String),
}

pub fn head_sha(repo_path: &Path) -> Result<GitSha, GitError> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            repo_path
                .to_str()
                .ok_or(GitError::Encoding("non-UTF-8 path".into()))?,
            "rev-parse",
            "HEAD",
        ])
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        return Err(GitError::NoHead);
    }

    let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
    GitSha::parse(sha).map_err(|_| GitError::InvalidId)
}

/// Extract human-authored hunks from working tree vs HEAD (or base commit).
/// Returns salted HMAC hashes of each hunk for privacy-preserving verification.
pub fn extract_human_hunks(
    repo_path: &Path,
    base_commit: Option<&GitSha>,
    pepper: &[u8],
) -> Result<Vec<SaltedHmacHunkHash>, GitError> {
    let mut args = vec!["diff", "--no-color"];

    if let Some(base) = base_commit {
        args.push(&base.0);
    }

    // Get diff with hunk headers
    args.push("--");
    args.push(".");

    let output = std::process::Command::new("git")
        .args([
            "-C",
            repo_path
                .to_str()
                .ok_or(GitError::Encoding("non-UTF-8 path".into()))?,
        ])
        .args(&args)
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let diff_output = String::from_utf8_lossy(&output.stdout);
    parse_hunks_from_diff(&diff_output, pepper)
}

fn parse_hunks_from_diff(diff: &str, pepper: &[u8]) -> Result<Vec<SaltedHmacHunkHash>, GitError> {
    let mut hunks = Vec::new();
    let mut current_hunk = String::new();
    let mut in_hunk = false;

    for line in diff.lines() {
        if line.starts_with("@@") {
            if in_hunk && !current_hunk.is_empty() {
                if let Ok(hash) = hunk_hash(&current_hunk, pepper) {
                    hunks.push(hash);
                }
            }
            current_hunk = line.to_string() + "\n";
            in_hunk = true;
        } else if in_hunk {
            current_hunk.push_str(line);
            current_hunk.push('\n');
        }
    }

    // Don't forget the last hunk
    if in_hunk && !current_hunk.is_empty() {
        if let Ok(hash) = hunk_hash(&current_hunk, pepper) {
            hunks.push(hash);
        }
    }

    Ok(hunks)
}

fn hunk_hash(hunk_text: &str, pepper: &[u8]) -> Result<SaltedHmacHunkHash, GitError> {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(pepper).map_err(|e| GitError::Hmac(e.to_string()))?;
    mac.update(hunk_text.as_bytes());
    let hash = mac.finalize().into_bytes();
    let hex = hex::encode(hash);
    SaltedHmacHunkHash::parse(hex).map_err(|_| GitError::Encoding("invalid hunk hash".into()))
}

/// Get the list of files changed in the working tree since base_commit (or HEAD).
pub fn changed_files(
    repo_path: &Path,
    base_commit: Option<&GitSha>,
) -> Result<Vec<std::path::PathBuf>, GitError> {
    let mut args = vec!["diff", "--name-only"];

    if let Some(base) = base_commit {
        args.push(&base.0);
    }

    args.push("--");
    args.push(".");

    let output = std::process::Command::new("git")
        .args([
            "-C",
            repo_path
                .to_str()
                .ok_or(GitError::Encoding("non-UTF-8 path".into()))?,
        ])
        .args(&args)
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let files_output = String::from_utf8_lossy(&output.stdout);
    Ok(files_output
        .lines()
        .filter(|l| !l.is_empty())
        .map(std::path::PathBuf::from)
        .collect())
}

/// Get the base commit for a session (first commit after session start, or HEAD~1)
pub fn session_base_commit(
    repo_path: &Path,
    session_start_millis: i64,
) -> Result<Option<GitSha>, GitError> {
    // Get commits since session start, in chronological order
    let since = format!("--since=@{}", session_start_millis / 1000);

    let output = std::process::Command::new("git")
        .args([
            "-C",
            repo_path
                .to_str()
                .ok_or(GitError::Encoding("non-UTF-8 path".into()))?,
        ])
        .args(["log", "--reverse", "--format=%H", &since, "--", "."])
        .output()
        .map_err(|e| GitError::Command(e.to_string()))?;

    if !output.status.success() {
        return Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let commits = String::from_utf8_lossy(&output.stdout);
    let first_commit = commits.lines().next();

    if let Some(sha) = first_commit {
        GitSha::parse(sha.to_string())
            .map(Some)
            .map_err(|_| GitError::InvalidId)
    } else {
        // No commits in session, return HEAD~1
        let output = std::process::Command::new("git")
            .args([
                "-C",
                repo_path
                    .to_str()
                    .ok_or(GitError::Encoding("non-UTF-8 path".into()))?,
            ])
            .args(["rev-parse", "HEAD~1"])
            .output()
            .map_err(|e| GitError::Command(e.to_string()))?;

        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
            GitSha::parse(sha)
                .map(Some)
                .map_err(|_| GitError::InvalidId)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn init_repo(dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        std::process::Command::new("git")
            .args(["init", dir.to_str().unwrap()])
            .output()?;

        std::process::Command::new("git")
            .args([
                "-C",
                dir.to_str().unwrap(),
                "config",
                "user.name",
                "Test User",
            ])
            .output()?;
        std::process::Command::new("git")
            .args([
                "-C",
                dir.to_str().unwrap(),
                "config",
                "user.email",
                "test@example.com",
            ])
            .output()?;
        Ok(())
    }

    #[test]
    fn extracts_hunks_from_working_tree() {
        let dir = tempdir().unwrap();
        init_repo(dir.path()).unwrap();

        fs::write(dir.path().join("file.txt"), "line 1\nline 2\nline 3\n").unwrap();
        std::process::Command::new("git")
            .args(["-C", dir.path().to_str().unwrap(), "add", "file.txt"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                dir.path().to_str().unwrap(),
                "commit",
                "-m",
                "initial",
            ])
            .output()
            .unwrap();

        fs::write(
            dir.path().join("file.txt"),
            "line 1\nline 2 modified\nline 3\nline 4\n",
        )
        .unwrap();

        let pepper = b"test-pepper-32-bytes-long!!!!!!!!!";
        let hunks = extract_human_hunks(dir.path(), None, pepper).unwrap();
        assert!(!hunks.is_empty(), "should extract at least one hunk");
    }
}
