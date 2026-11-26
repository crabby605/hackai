use crate::{api, harness, read_stored_token, state, tokio_rt};
use dialoguer::Confirm;
use std::{fs, io, path::Path};

pub fn cmd_track(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let s = state::state_dir(&r);
    let first_setup = !s.exists();

    if !Confirm::new()
        .with_prompt("Track this directory?")
        .default(true)
        .interact()
        .map_err(io::Error::other)?
    {
        println!("Nothing changed.");
        return Ok(());
    }

    let include_dashboard = if local_only {
        false
    } else if dashboard {
        true
    } else {
        println!(
            "Server upload sends time, tool/test outcomes, and a derived dialogue trace (for example: Your action and the agent result). It never sends raw prompts, replies, source code, or file paths."
        );
        println!(
            "The optional MCP feedback action stores a short, explicitly requested coaching note for your account; it does not upload conversation text."
        );
        Confirm::new()
            .with_prompt("Opt in to server upload and mandatory background sync?")
            .default(false)
            .interact()
            .map_err(io::Error::other)?
    };

    println!(
        "Plan: create local tracking state at {}.",
        state::safe_terminal(&s.display().to_string())
    );
    fs::create_dir_all(&s)?;
    fs::write(s.join("tracked"), b"v1\n")?;
    state::write_server_upload_consent(&r, include_dashboard)?;
    state::audit(&r, "track")?;

    if first_setup {
        // Local adapters are always safe to install: their payloads are
        // reduced locally and no data leaves the machine from this step.
        harness::install_adapters(&r)?;
        let server_url = std::env::var("HACKAI_SERVER_URL")
            .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
        match api::fetch_annotation_policy(&server_url) {
            Ok(policy) => harness::install(&r, harness::InstallScope::Project, &policy)?,
            Err(error) => eprintln!(
                "Adapters installed; optional annotation skill will retry when online: {error}"
            ),
        }
        if let Err(error) = crate::commands::collect::cmd_collect(&r) {
            eprintln!("Initial local collection deferred: {error}");
        }
    }

    if include_dashboard {
        if let Err(error) = crate::commands::sync::install_background_worker(&r) {
            eprintln!("Could not install background sync: {error}");
        }
        println!(
            "Background sync is ready. Run `hackai login` when you want to authorize server access."
        );
    } else {
        println!(
            "Tracking was not enabled for server upload. Run `hackai track --dashboard` when you want to opt in."
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
