// Git operations
use std::io;
use std::path::Path;
use std::process::Command;

pub fn git_remote_url(repo_root: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args([
            "-C",
            repo_root
                .to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 path"))?,
            "remote",
            "get-url",
            "origin",
        ])
        .output()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("git remote: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no git remote origin",
        ))
    }
}

pub fn git_head_sha(repo_root: &Path) -> io::Result<String> {
    let output = Command::new("git")
        .args([
            "-C",
            repo_root
                .to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 path"))?,
            "rev-parse",
            "HEAD",
        ])
        .output()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("git rev-parse: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "no HEAD commit"))
    }
}
