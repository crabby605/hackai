//! Shared queue processing logic.

use crate::{api, state};
use std::{io, path::Path};

fn backoff_seconds(attempts: u32) -> u64 {
    let shift = attempts.saturating_sub(1).min(6);
    60_u64.saturating_mul(1_u64 << shift).min(3_600)
}

pub fn process_queue(root: &Path, api_token: &str, server_url: &str) -> io::Result<()> {
    let queue = state::read_queue(root)?;
    if queue.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }
    let mut remaining = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    for mut item in queue {
        if now < item.next_attempt_at {
            remaining.push(item);
            continue;
        }
        let repo_path = Path::new(&item.repo_root);
        // The batch ID identifies this exact snapshot, even if local work
        // changed after a response was lost. New work gets a new batch.
        match api::sync_to_server(server_url, api_token, item.payload.clone()) {
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
                item.attempts = item.attempts.saturating_add(1);
                item.next_attempt_at = now.saturating_add(backoff_seconds(item.attempts));
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

#[cfg(test)]
mod tests {
    use super::backoff_seconds;

    #[test]
    fn retries_back_off_and_cap_without_discarding_work() {
        assert_eq!(backoff_seconds(1), 60);
        assert_eq!(backoff_seconds(2), 120);
        assert_eq!(backoff_seconds(7), 3_600);
        assert_eq!(backoff_seconds(100), 3_600);
    }
}
