//! Installation and integration setup for various coding harnesses.

use crate::state;
use std::{io, path::Path};

pub fn run_checkpoint_nudge(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let config = state::tracker_config(&r)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let mut checkpoint = state::read_checkpoint_state(&r)?;
    checkpoint.turns_since_nudge = checkpoint.turns_since_nudge.saturating_add(1);
    let due = checkpoint.turns_since_nudge >= config.checkpoint_turn_interval
        && now.saturating_sub(checkpoint.last_nudge_unix) >= 1_200;
    if due {
        checkpoint.turns_since_nudge = 0;
        checkpoint.last_nudge_unix = now;
    }
    state::write_checkpoint_state(&r, &checkpoint)?;
    state::audit(&r, "checkpoint_nudge")?;
    if due {
        println!(
            "Checkpoint due after the current task. If evidence exists, use the HackAI annotation skill and MCP annotate tool. Default to delegated unless cited user edits or corrections support another label. Never include prompt text or award time."
        );
    }
    Ok(())
}

pub fn set_checkpoint_interval(repo: &Path, value: u16) -> io::Result<()> {
    if !(5..=100).contains(&value) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "checkpoint turn interval must be between 5 and 100",
        ));
    }
    let r = state::root(repo)?;
    state::write_config(
        &r,
        &state::TrackerConfig {
            checkpoint_turn_interval: value,
        },
    )?;
    state::audit(&r, "config_checkpoint_turn_interval")?;
    println!("Checkpoint turn interval set to {value}.");
    Ok(())
}
