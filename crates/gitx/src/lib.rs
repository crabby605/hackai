#![forbid(unsafe_code)]
//! Git operations using CLI for maximum compatibility and simplicity.
//! Uses `git` command-line tool for diff extraction and hunk hashing.

use hackai_schema::{GitSha, SaltedHmacHunkHash};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::path::Path;
use thiserror::Error;

/// Privacy-preserving facts about a commit.  Author names, subjects, paths,
/// and diff contents intentionally do not leave gitx.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitSummary {
    pub sha: GitSha,
    pub tree: String,
    pub committed_at_millis: i64,
    pub files_changed: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
}

/// A point-in-time summary of changes which have not been committed.  It is a
/// polling primitive, not authorship proof: another collector must exclude
/// known agent writes before this is credited as a human edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkingTreeSummary {
    pub head: Option<GitSha>,
    pub files_changed: u32,
    pub lines_added: u32,
    pub lines_deleted: u32,
    pub untracked_files: u32,
}

impl WorkingTreeSummary {
    pub const fn is_clean(&self) -> bool {
        self.files_changed == 0 && self.untracked_files == 0
    }
}

/// Return commits at or after `since_millis`, with only derived diff counts.
/// This deliberately performs local git reads only; it never fetches or
/// contacts a remote.
pub fn commits_since(repo_path: &Path, since_millis: i64) -> Result<Vec<CommitSummary>, GitError> {
    let since = format!("--since=@{}", since_millis.div_euclid(1000));
    let output = git_output(repo_path, &["log", "--format=%H%x1f%T%x1f%ct", &since])?;
    let mut commits = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.is_empty())
    {
        let mut fields = line.split('\u{1f}');
        let sha = fields.next().ok_or(GitError::InvalidId)?;
        let tree = fields.next().ok_or(GitError::InvalidId)?;
        let seconds = fields
            .next()
            .ok_or(GitError::InvalidId)?
            .parse::<i64>()
            .map_err(|_| GitError::InvalidId)?;
        let sha = GitSha::parse(sha.to_owned()).map_err(|_| GitError::InvalidId)?;
        if !(40..=64).contains(&tree.len()) || !tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(GitError::InvalidId);
        }
        let (files_changed, lines_added, lines_deleted) = commit_numstat(repo_path, &sha)?;
        commits.push(CommitSummary {
            sha,
            tree: tree.to_owned(),
            committed_at_millis: seconds.saturating_mul(1000),
            files_changed,
            lines_added,
            lines_deleted,
        });
    }
    Ok(commits)
}

/// Summarise staged and unstaged changes without exposing file names or code.
pub fn working_tree_summary(repo_path: &Path) -> Result<WorkingTreeSummary, GitError> {
    let unstaged = git_output(repo_path, &["diff", "--numstat"])?;
    let staged = git_output(repo_path, &["diff", "--cached", "--numstat"])?;
    let (mut files_changed, mut lines_added, mut lines_deleted) = numstat(&unstaged.stdout);
    let (staged_files, staged_added, staged_deleted) = numstat(&staged.stdout);
    files_changed = files_changed.saturating_add(staged_files);
    lines_added = lines_added.saturating_add(staged_added);
    lines_deleted = lines_deleted.saturating_add(staged_deleted);

    let status = git_output(
        repo_path,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    let untracked_files = String::from_utf8_lossy(&status.stdout)
        .lines()
        .filter(|line| line.starts_with("??"))
        .count()
        .try_into()
        .unwrap_or(u32::MAX);

    Ok(WorkingTreeSummary {
        head: head_sha(repo_path).ok(),
        files_changed,
        lines_added,
        lines_deleted,
        untracked_files,
    })
}

fn commit_numstat(repo_path: &Path, sha: &GitSha) -> Result<(u32, u32, u32), GitError> {
    let output = git_output(repo_path, &["show", "--format=", "--numstat", &sha.0])?;
    Ok(numstat(&output.stdout))
}

fn numstat(output: &[u8]) -> (u32, u32, u32) {
    let mut files = 0_u32;
    let mut added = 0_u32;
    let mut deleted = 0_u32;
    for line in String::from_utf8_lossy(output).lines() {
        let mut fields = line.split('\t');
        let Some(additions) = fields.next() else {
            continue;
        };
        let Some(deletions) = fields.next() else {
            continue;
        };
        // Binary entries are `-`; count the file but not invented line totals.
        if fields.next().is_none() {
            continue;
        }
        files = files.saturating_add(1);
        added = added.saturating_add(additions.parse::<u32>().unwrap_or(0));
        deleted = deleted.saturating_add(deletions.parse::<u32>().unwrap_or(0));
    }
    (files, added, deleted)
}

fn git_output(repo_path: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
    let root = repo_path
        .to_str()
        .ok_or(GitError::Encoding("non-UTF-8 path".into()))?;
    let output = std::process::Command::new("git")
        .args(["-C", root])
        .args(args)
        .output()
        .map_err(|error| GitError::Command(error.to_string()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

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
            if in_hunk
                && !current_hunk.is_empty()
                && let Ok(hash) = hunk_hash(&current_hunk, pepper)
            {
                hunks.push(hash);
            }
            current_hunk = line.to_string() + "\n";
            in_hunk = true;
        } else if in_hunk {
            current_hunk.push_str(line);
            current_hunk.push('\n');
        }
    }

    // Don't forget the last hunk
    if in_hunk
        && !current_hunk.is_empty()
        && let Ok(hash) = hunk_hash(&current_hunk, pepper)
    {
        hunks.push(hash);
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
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

    #[test]
    fn collects_commit_and_worktree_counts_without_paths() {
        let dir = tempdir().unwrap();
        init_repo(dir.path()).unwrap();
        fs::write(dir.path().join("private-name.txt"), "one\ntwo\n").unwrap();
        std::process::Command::new("git")
            .args(["-C", dir.path().to_str().unwrap(), "add", "."])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-C",
                dir.path().to_str().unwrap(),
                "commit",
                "-m",
                "private subject",
            ])
            .output()
            .unwrap();

        let commits = commits_since(dir.path(), 0).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].lines_added, 2);
        assert_eq!(commits[0].files_changed, 1);

        fs::write(dir.path().join("private-name.txt"), "one\ntwo\nthree\n").unwrap();
        fs::write(dir.path().join("untracked-secret.txt"), "x\n").unwrap();
        let summary = working_tree_summary(dir.path()).unwrap();
        assert_eq!(summary.lines_added, 1);
        assert_eq!(summary.files_changed, 1);
        assert_eq!(summary.untracked_files, 1);
    }
}
