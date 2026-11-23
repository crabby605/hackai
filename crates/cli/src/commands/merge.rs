use crate::{api, read_stored_token, state, tokio_rt};
use dialoguer::{Confirm, Input};
use std::{io, path::Path};

pub fn cmd_merge(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());

    let token = read_stored_token(&r)
        .ok_or_else(|| io::Error::other("not authenticated; run `hackai login`"))?;

    let rt = tokio_rt()?;
    let projects = rt.block_on(api::api_get_projects(&server_url, &token))?;

    if projects.is_empty() {
        println!("No projects found on dashboard.");
        return Ok(());
    }

    println!("Your projects:");
    for (i, p) in projects.iter().enumerate() {
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let total = p.get("total_seconds").and_then(|v| v.as_i64()).unwrap_or(0);
        println!("  {}. {} ({}h)", i + 1, name, total / 3600);
    }

    let source_idx: usize = Input::new()
        .with_prompt("Select SOURCE project to merge (number)")
        .interact_text()
        .map_err(io::Error::other)?;
    if source_idx == 0 || source_idx > projects.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid selection",
        ));
    }
    let source = &projects[source_idx - 1];
    let source_id = source
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("source project missing id"))?;

    let target_idx: usize = Input::new()
        .with_prompt("Select TARGET project to merge INTO (number)")
        .interact_text()
        .map_err(io::Error::other)?;
    if target_idx == 0 || target_idx > projects.len() || target_idx == source_idx {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid selection",
        ));
    }
    let target = &projects[target_idx - 1];
    let target_id = target
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("target project missing id"))?;

    let source_name = source
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("source project missing name"))?;
    let target_name = target
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("target project missing name"))?;

    if !Confirm::new()
        .with_prompt(format!(
            "Merge '{}' into '{}'? This cannot be undone.",
            source_name, target_name
        ))
        .default(false)
        .interact()
        .map_err(io::Error::other)?
    {
        println!("Cancelled.");
        return Ok(());
    }

    let result = rt.block_on(api::api_merge_projects(
        &server_url,
        &token,
        source_id,
        target_id,
    ))?;
    println!("Merged successfully!");
    println!(
        "  Episodes moved: {}",
        result
            .get("episodes_moved")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    );
    println!(
        "  Credited seconds moved: {}",
        result
            .get("credited_seconds_moved")
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
    );
    Ok(())
}
