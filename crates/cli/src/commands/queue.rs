use crate::commands::queue_process::process_queue;
use crate::{read_stored_token, state};
use std::{io, path::Path};

pub fn cmd_queue_list(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let queue = state::read_queue(&r)?;
    if queue.is_empty() {
        println!("Queue is empty.");
    } else {
        println!("Pending sync operations:");
        for (i, item) in queue.iter().enumerate() {
            println!(
                "  {}. {} (attempts: {})",
                i + 1,
                item.project_name,
                item.attempts
            );
        }
    }
    Ok(())
}

pub fn cmd_queue_clear(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    state::write_queue(&r, &[])?;
    println!("Queue cleared.");
    Ok(())
}

pub fn cmd_queue_process(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let token = read_stored_token(&r).ok_or_else(|| io::Error::other("not authenticated"))?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
    process_queue(&r, &token, &server_url)
}
