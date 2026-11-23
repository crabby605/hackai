//! Shared queue processing logic.

use crate::{api, state};
use std::{io, path::Path};

pub fn process_queue(root: &Path, api_token: &str, server_url: &str) -> io::Result<()> {
    let queue = state::read_queue(root)?;
    if queue.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }
    let mut remaining = Vec::new();
    for mut item in queue {
        let repo_path = Path::new(&item.repo_root);
        let remote_url = crate::git::git_remote_url(repo_path).unwrap_or_default();
        let head_sha = crate::git::git_head_sha(repo_path).unwrap_or_default();
        let mut payload = item.payload.clone();
        payload.remote_url = Some(remote_url);
        payload.head_sha = Some(head_sha);
        payload.annotations = state::read_local_annotations(repo_path);
        payload.checkpoints = state::read_local_checkpoints(repo_path);
        payload.episodes = state::read_local_episodes(repo_path);

        match api::sync_to_server(server_url, api_token, payload) {
            Ok(body) => {
                println!(
                    "Synced {} · {} episode(s)",
                    body.get("project_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-"),
                    body.get("episodes_synced")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                );
                state::audit(repo_path, "sync")?;
            }
            Err(e) => {
                eprintln!("Sync failed for {}: {e}", item.project_name);
                item.attempts += 1;
                // Keep failed work durable; transient outages can last longer
                // than five attempts and must not silently discard activity.
                remaining.push(item);
            }
        }
    }
    state::write_queue(root, &remaining)?;
    if !remaining.is_empty() {
        println!(
            "{} item(s) remain in queue (will retry on next sync).",
            remaining.len()
        );
    }
    Ok(())
}