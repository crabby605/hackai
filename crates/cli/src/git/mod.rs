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
        .map_err(|e| io::Error::other(format!("git remote: {e}")))?;
    if output.status.success() {
        Ok(sanitize_remote_url(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no git remote origin",
        ))
    }
}

/// Strip HTTP credentials and query/fragment data before a remote leaves the device.
fn sanitize_remote_url(url: &str) -> String {
    let Some((scheme, remainder)) = url.split_once("://") else {
        return url.to_owned();
    };
    if !matches!(scheme, "http" | "https") {
        return url.to_owned();
    }
    let without_query = remainder.split(['?', '#']).next().unwrap_or(remainder);
    let (authority, path) = without_query
        .split_once('/')
        .map_or((without_query, ""), |(host, path)| (host, path));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if path.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{path}")
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
        .map_err(|e| io::Error::other(format!("git rev-parse: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "no HEAD commit"))
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_remote_url;

    #[test]
    fn strips_remote_credentials_and_query_data() {
        assert_eq!(
            sanitize_remote_url("https://user:secret@github.com/org/repo.git?token=secret"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            sanitize_remote_url("https://github.com/org/repo.git"),
            "https://github.com/org/repo.git"
        );
        assert_eq!(
            sanitize_remote_url("git@github.com:org/repo.git"),
            "git@github.com:org/repo.git"
        );
    }
}
