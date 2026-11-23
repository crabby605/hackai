use crate::commands::queue_process::process_queue;
use crate::{api, read_stored_token, state};
use std::{io, path::Path};

pub fn cmd_sync(repo: &Path, force: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());

    let token = read_stored_token(&r);

    let payload = api::build_sync_payload(&r)?;

    if let Some(ref token) = token {
        match api::sync_to_server(&server_url, token, payload.clone()) {
            Ok(body) => {
                println!(
                    "Synced project {} · {} episode(s)",
                    body.get("project_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-"),
                    body.get("episodes_synced")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                );
                if let Some(url) = body.get("github_url").and_then(|v| v.as_str()) {
                    println!("Git origin: {url}");
                }
                state::audit(&r, "sync")?;

                let queue = state::read_queue(&r)?;
                if !queue.is_empty() {
                    println!("Processing {} queued sync(s)...", queue.len());
                    process_queue(&r, token, &server_url)?;
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!("Sync failed: {e}");
                state::enqueue_sync(&r, &r, payload.clone())?;
                println!("Sync queued locally (will retry on next successful sync).");
            }
        }
    }

    if force || token.is_none() {
        state::enqueue_sync(&r, &r, payload)?;
        if token.is_none() {
            println!(
                "Not authenticated. Sync queued locally. Run `hackai login` then `hackai sync` to send."
            );
        } else {
            println!("Sync queued locally (will retry on next successful sync).");
        }
    }
    Ok(())
}

pub fn cmd_api(repo: &Path, method: String, path: String, body: Option<String>) -> io::Result<()> {
    let root = state::root(repo)?;
    let config = api::read_local_config(&root)?;
    let token = std::env::var("HACKAI_API_KEY")
        .ok()
        .or(config.api_key)
        .or_else(|| read_stored_token(&root))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no API key; set HACKAI_API_KEY, .hackai.cfg, or run hackai login",
            )
        })?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .ok()
        .or(config.server_url)
        .unwrap_or_else(|| "https://hackai.crabby605.hackclub.app".into());
    let body = body
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid JSON body: {error}"),
            )
        })?;
    let response = api::api_request(&server_url, &token, &method, &path, body)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&response).map_err(io::Error::other)?
    );
    Ok(())
}
