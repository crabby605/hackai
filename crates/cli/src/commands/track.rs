use crate::{api, harness, state, read_stored_token, tokio_rt};
use dialoguer::{Confirm, Select};
use std::{fs, io, path::Path};

pub fn cmd_track(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let s = state::state_dir(&r);
    let first_setup = !s.exists();

    let include_dashboard = if local_only {
        false
    } else if dashboard {
        true
    } else {
        Confirm::new()
            .with_prompt("Include this project in your dashboard (requires auth)?")
            .default(false)
            .interact()
            .map_err(io::Error::other)?
    };

    println!(
        "Plan: create local-only {} and append a post-commit hook stanza.",
        state::safe_terminal(&s.display().to_string())
    );
    fs::create_dir_all(&s)?;
    fs::write(s.join("tracked"), b"v1\n")?;
    state::audit(&r, "track")?;

    if first_setup {
        let install = Confirm::new()
            .with_prompt("Install HackAI hooks, automatic MCP, and the annotation skill?")
            .default(true)
            .interact()
            .map_err(io::Error::other)?;
        if install {
            let options = ["This project only", "Globally for my user"];
            let selected = Select::new()
                .with_prompt("Where should HackAI install the integration?")
                .items(&options)
                .default(0)
                .interact()
                .map_err(io::Error::other)?;
            let server_url = std::env::var("HACKAI_SERVER_URL")
                .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
            let policy = api::fetch_annotation_policy(&server_url).map_err(|error| {
                io::Error::other(format!("could not fetch the server policy: {error}"))
            })?;
            harness::install(
                &r,
                if selected == 0 {
                    harness::InstallScope::Project
                } else {
                    harness::InstallScope::Global
                },
                &policy,
            )?;
        }
    }

    if include_dashboard {
        let server_url = std::env::var("HACKAI_SERVER_URL")
            .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
        if let Some(token) = read_stored_token(&r) {
            let rt = tokio_rt()?;
            rt.block_on(async {
                let payload = api::build_sync_payload(&r).unwrap_or_else(|_| api::SyncPayload {
                    batch_id: Some(uuid::Uuid::new_v4().to_string()),
                    annotation_policy_sha256: String::new(),
                    cli_release_sha256: String::new(),
                    adapter_sha256: std::collections::BTreeMap::new(),
                    remote_url: None,
                    head_sha: None,
                    annotations: vec![],
                    checkpoints: vec![],
                    episodes: vec![],
                    prompt_metrics: vec![],
                    prompt_events: vec![],
                    hardware_hashes: std::collections::BTreeMap::new(),
                    environment_class: "unknown".to_owned(),
                });
                let _ = api::sync_to_server(&server_url, &token, payload);
            });
            println!("Project queued for dashboard sync.");
        } else {
            println!("Not authenticated. Run `hackai login` to enable dashboard sync.");
        }
    } else {
        println!(
            "Tracking enabled for this repository (local-only). No paths or transcript text are stored."
        );
    }
    Ok(())
}

pub fn cmd_untrack(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let s = state::state_dir(&r);

    if !s.exists() {
        println!("Not tracking this repository.");
        return Ok(());
    }

    let delete_from_dashboard = if local_only {
        false
    } else if dashboard {
        true
    } else {
        Confirm::new()
            .with_prompt("Also delete this project from your dashboard?")
            .default(false)
            .interact()
            .map_err(io::Error::other)?
    };

    if delete_from_dashboard {
        let server_url = std::env::var("HACKAI_SERVER_URL")
            .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
        if let Some(token) = read_stored_token(&r) {
            let project_name = r.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let rt = tokio_rt()?;
            let projects = rt
                .block_on(api::api_get_projects(&server_url, &token))
                .unwrap_or_default();
            if let Some(proj) = projects
                .iter()
                .find(|p| p.get("name").and_then(|v| v.as_str()) == Some(project_name))
                && let Some(id) = proj.get("id").and_then(|v| v.as_str())
            {
                let _ = rt.block_on(api::api_delete_project(&server_url, &token, id));
                println!("Project deleted from dashboard.");
            }
        } else {
            println!("Not authenticated. Skipping dashboard deletion.");
        }
    }

    fs::remove_dir_all(&s)?;
    println!("Local HackAI state removed; existing git hooks were not changed.");
    Ok(())
}

pub fn cmd_status(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    state::audit(&r, "status")?;
    println!(
        "{}",
        if state::state_dir(&r).join("tracked").exists() {
            "tracked (local-only)"
        } else {
            "not tracked"
        }
    );
    let claude = state::discover_claude_transcripts(&r).unwrap_or_default();
    let codex = state::discover_codex_rollouts().unwrap_or_default();
    let opencode = state::discover_opencode_database();
    println!(
        "Available collectors: Claude Code {} transcript(s), Codex {} rollout(s), OpenCode {}",
        claude.len(),
        codex.len(),
        if opencode.is_some() {
            "database found"
        } else {
            "not found"
        }
    );
    Ok(())
}

pub fn cmd_audit(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    state::audit(&r, "audit")?;
    println!(
        "Audit log location: {}",
        state::safe_terminal(
            &state::state_dir(&r)
                .join("audit.jsonl")
                .display()
                .to_string()
        )
    );
    Ok(())
}