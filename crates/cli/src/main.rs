#![forbid(unsafe_code)]
//! Local-first AI coding time tracker
//!
//! Modular structure:
//! - state: local paths, config, queue, audit, checkpoint
//! - api: HTTP client functions for server communication
//! - git: git command wrappers
//! - mcp: stdio MCP server with annotate + PESDO tools

mod api;
mod git;
mod harness;
mod mcp;
mod prompt_classifier;
mod prompt_model;
mod state;

use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input};
use hackai_parser::{parse_claude_jsonl, visit_claude_prompts};
use hackai_scorer::productive_episodes;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const KEYRING_SERVICE: &str = "hackai";
const KEYRING_USER: &str = "api_token";

fn token_entry(root: &Path) -> Entry {
    let repo_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");
    Entry::new(KEYRING_SERVICE, &format!("{KEYRING_USER}_{repo_name}")).unwrap()
}

fn read_stored_token(root: &Path) -> Option<String> {
    token_entry(root)
        .get_password()
        .ok()
        .filter(|t| !t.trim().is_empty())
}

fn write_stored_token(root: &Path, token: &str) -> io::Result<()> {
    token_entry(root)
        .set_password(token)
        .map_err(io::Error::other)
}

fn clear_stored_token(root: &Path) -> io::Result<()> {
    let _ = token_entry(root).delete_credential();
    Ok(())
}

#[derive(Parser)]
#[command(name = "hackai", version, about = "Local-first AI coding time tracker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start tracking a repository (interactive: asks about dashboard sync)
    Track {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Skip interactive prompts, track locally only
        #[arg(long)]
        local_only: bool,
        /// Also sync to dashboard (requires auth)
        #[arg(long)]
        dashboard: bool,
    },
    /// Stop tracking a repository (interactive: asks about dashboard deletion)
    Untrack {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Skip interactive prompts, remove local only
        #[arg(long)]
        local_only: bool,
        /// Also delete from dashboard (requires auth)
        #[arg(long)]
        dashboard: bool,
    },
    Status {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    Audit {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    /// Remove local state only (alias for untrack --local-only)
    Purge {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    /// Remove local state only (alias for untrack --local-only)
    Uninstall {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    Export {
        #[arg(long)]
        preview: bool,
    },
    Sync {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Force sync even if queue is empty
        #[arg(long)]
        force: bool,
    },
    #[cfg(feature = "dev-tools")]
    Explain,
    /// Analyze a Claude JSONL transcript into privacy-preserving struggle episodes.
    #[cfg(feature = "dev-tools")]
    Episodes {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Local JSONL transcript. Its raw text is never persisted by HackAI.
        #[arg(long)]
        transcript: PathBuf,
        #[arg(long, default_value = "default")]
        session: String,
    },
    /// Authenticate with the server (API token or browser pairing)
    Login {
        #[arg(help = "API token to store locally; omitted opens browser pairing")]
        token: Option<String>,
        /// Explicit API-key form (also accepts HACKAI_API_KEY).
        #[arg(long, conflicts_with = "token")]
        api_key: Option<String>,
        /// Kept for backwards compatibility; browser pairing is now the default
        #[arg(long)]
        device: bool,
    },
    /// Log out (remove stored token)
    Logout,
    /// Show current authentication status and user info
    Whoami,
    Annotate {
        #[arg(default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        window: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        user_role: String,
        #[arg(long, value_delimiter = ',')]
        evidence: Vec<String>,
        #[arg(long)]
        chapter_title: Option<String>,
    },
    /// Run the local stdio MCP server for advisory annotations.
    Mcp,
    /// Classify prompt tone locally from standard input; the prompt is never stored or synced.
    #[cfg(feature = "dev-tools")]
    ClassifyPrompt,
    /// Train a local prompt-clarity model from consented JSONL examples.
    #[cfg(feature = "dev-tools")]
    TrainPromptModel {
        #[arg(long, default_value = "ml/prompt_clarity_seed.jsonl")]
        dataset: PathBuf,
        #[arg(long, default_value = "ml/prompt-model.bootstrap.json")]
        output: PathBuf,
    },
    /// Generate a varied, clearly-marked synthetic prompt dataset locally.
    #[cfg(feature = "dev-tools")]
    GeneratePromptDataset {
        #[arg(long, default_value = "ml/prompt_clarity_synthetic_v2.jsonl")]
        output: PathBuf,
        /// Number of examples for each of new, developing, and proficient.
        #[arg(long, default_value_t = 500)]
        per_level: usize,
    },
    /// Classify one pasted prompt and save it locally for future model tuning.
    #[cfg(feature = "dev-tools")]
    PromptLab {
        /// Local-only JSONL file. This default is ignored by Git.
        #[arg(long, default_value = "ml/prompt-lab.local.jsonl")]
        log: PathBuf,
        /// Optional human review label: new, developing, or proficient.
        #[arg(long, value_parser = ["new", "developing", "proficient"])]
        proficiency_label: Option<String>,
        /// Optional human review label: low, medium, or high agent delegation.
        #[arg(long, value_parser = ["low", "medium", "high"])]
        delegation_label: Option<String>,
    },
    /// Set up MCP for all Coding IDEs and store a nonce.
    Install,
    /// Install and run privacy-preserving coding-harness adapters.
    Harness {
        #[command(subcommand)]
        command: HarnessCommand,
    },
    /// Emit a Claude Code checkpoint reminder when the local policy is due.
    #[command(hide = true)]
    CheckpointNudge {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    /// Set the repository-local checkpoint turn interval (5 through 100).
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Merge two projects (interactive: finds similar names)
    Merge {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    /// View and manage offline sync queue
    Queue {
        #[command(subcommand)]
        command: QueueCommand,
    },
}

#[derive(Subcommand)]
enum ConfigCommand {
    CheckpointTurnInterval {
        value: u16,
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
}

#[derive(Subcommand)]
enum QueueCommand {
    /// Show pending sync operations
    List,
    /// Clear the offline queue
    Clear,
    /// Process the queue now
    Process,
}

#[derive(Subcommand)]
enum HarnessCommand {
    /// Install project-local hooks for Codex, Claude Code, OpenCode, and Antigravity.
    Install {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    /// Internal hook entry point. Reads one harness JSON event from stdin.
    #[command(hide = true)]
    Hook {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        harness: String,
        #[arg(long)]
        event: String,
    },
    /// Run Aider through the local prompt-classification wrapper.
    Aider {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
}

// --- Command implementations ---

fn cmd_track(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let s = state::state_dir(&r);

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

    if include_dashboard {
        let server_url = std::env::var("HACKAI_SERVER_URL")
            .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
        if let Some(token) = read_stored_token(&r) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let _ = rt.block_on(async {
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

fn cmd_untrack(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
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
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let projects = rt
                .block_on(api::api_get_projects(&server_url, &token))
                .unwrap_or_default();
            if let Some(proj) = projects
                .iter()
                .find(|p| p.get("name").and_then(|v| v.as_str()) == Some(project_name))
            {
                if let Some(id) = proj.get("id").and_then(|v| v.as_str()) {
                    let _ = rt.block_on(api::api_delete_project(&server_url, &token, id));
                    println!("Project deleted from dashboard.");
                }
            }
        } else {
            println!("Not authenticated. Skipping dashboard deletion.");
        }
    }

    fs::remove_dir_all(&s)?;
    println!("Local HackAI state removed; existing git hooks were not changed.");
    Ok(())
}

fn cmd_status(repo: &Path) -> io::Result<()> {
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
    Ok(())
}

fn cmd_audit(repo: &Path) -> io::Result<()> {
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

fn cmd_export(preview: bool) -> io::Result<()> {
    if !preview {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export requires --preview until privacy-export requirements are confirmed",
        ));
    }
    println!(
        "Preview only: export is unavailable until Hack Club's deletion/export contract is published."
    );
    Ok(())
}

fn cmd_login(token: Option<String>, api_key: Option<String>, _device: bool) -> io::Result<()> {
    let r = state::root(Path::new("."))?;

    if let Some(token) = api_key
        .or(token)
        .or_else(|| std::env::var("HACKAI_API_KEY").ok())
    {
        let token = token.trim();
        if token.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "api token must not be empty",
            ));
        }
        write_stored_token(&r, token)?;
        println!("API token stored in system keyring. Run `hackai sync` to sync.");
        return Ok(());
    }

    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".to_string());

    println!("Starting browser authorization...");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let pair = rt.block_on(api::api_cli_pair(&server_url))?;

    let device_code = pair
        .get("device_code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("pairing response missing device_code"))?;
    let user_code = pair
        .get("user_code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("pairing response missing user_code"))?;
    let verification_uri = pair
        .get("verification_uri")
        .and_then(|v| v.as_str())
        .ok_or_else(|| io::Error::other("pairing response missing verification_uri"))?;
    let expires_in = pair
        .get("expires_in")
        .and_then(|v| v.as_u64())
        .unwrap_or(600);

    println!("\nPlease authorize the CLI in your browser:");
    println!("  Code: {}", user_code);
    println!("  URL:  {}", verification_uri);
    println!("  Expires in: {} seconds", expires_in);

    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "start"
    } else {
        "xdg-open"
    };
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(expires_in);
    let (status_tx, status_rx) = std::sync::mpsc::channel::<io::Result<Value>>();
    let poll_server = server_url.clone();
    let poll_device = device_code.to_string();
    std::thread::spawn(move || {
        let poll_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        let Ok(poll_rt) = poll_rt else { return };
        loop {
            if let Ok(value) =
                poll_rt.block_on(api::api_cli_pair_status(&poll_server, &poll_device))
            {
                let status = value
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("pending");
                if status != "pending" {
                    let _ = status_tx.send(Ok(value));
                    return;
                }
            }
            if start.elapsed() > timeout {
                let _ = status_tx.send(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "authorization timed out",
                )));
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    });
    let (input_tx, input_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let _ = input_tx.send(input);
        }
    });
    println!(
        "\nPress Enter to open the authorization page in your browser (or paste the URL yourself)."
    );
    println!("Waiting for authorization...");
    loop {
        if let Ok(input) = input_rx.try_recv() {
            let _ = input;
            let _ = std::process::Command::new(opener)
                .arg(verification_uri)
                .spawn();
            println!("Authorization page opened.");
        }
        let status = match status_rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(result) => result?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("authorization polling stopped"));
            }
        };
        let status_str = status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        match status_str {
            "approved" => {
                let token = status.get("token").and_then(|v| v.as_str()).unwrap();
                write_stored_token(&r, token)?;
                println!("\n✓ Authorized! API token stored in system keyring.");
                println!("Run `hackai sync` to sync your data.");
                return Ok(());
            }
            "pending" => {
                print!(".");
                std::io::stdout().flush().ok();
            }
            _ => {
                return Err(io::Error::other(format!(
                    "authorization failed: {}",
                    status_str
                )));
            }
        }
    }
}

fn cmd_logout() -> io::Result<()> {
    let r = state::root(Path::new("."))?;
    clear_stored_token(&r)?;
    println!("Logged out. Local API token removed from keyring.");
    Ok(())
}

fn cmd_whoami() -> io::Result<()> {
    let r = state::root(Path::new("."))?;
    let token = read_stored_token(&r);

    if token.is_none() {
        println!("Not authenticated. Run `hackai login` to authenticate.");
        return Ok(());
    }

    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".to_string());

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let user = rt.block_on(api::api_get_me(&server_url, token.as_ref().unwrap()))?;

    println!("Authenticated as:");
    println!(
        "  Username: {}",
        user.get("username").and_then(|v| v.as_str()).unwrap_or("-")
    );
    println!(
        "  Display name: {}",
        user.get("display_name")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
    );
    println!(
        "  Profile public: {}",
        user.get("profile_public")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    println!(
        "  Leaderboard opt-out: {}",
        user.get("leaderboard_opt_out")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    println!(
        "  Admin: {}",
        user.get("is_admin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    println!(
        "  Country: {}",
        user.get("country_code")
            .and_then(|v| v.as_str())
            .unwrap_or("-")
    );

    Ok(())
}

fn cmd_sync(repo: &Path, force: bool) -> io::Result<()> {
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

                let queue = state::read_queue(&r);
                if !queue.is_empty() {
                    println!("Processing {} queued sync(s)...", queue.len());
                    process_queue(&r, token, &server_url)?;
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!("Sync failed: {e}");
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

fn process_queue(root: &Path, api_token: &str, server_url: &str) -> io::Result<()> {
    let mut queue = state::read_queue(root);
    if queue.is_empty() {
        println!("Queue is empty.");
        return Ok(());
    }
    let mut remaining = Vec::new();
    for mut item in queue {
        let repo_path = Path::new(&item.repo_root);
        let remote_url = git::git_remote_url(repo_path).unwrap_or_default();
        let head_sha = git::git_head_sha(repo_path).unwrap_or_default();
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
                if item.attempts < 5 {
                    remaining.push(item);
                } else {
                    eprintln!("Max retries exceeded for {}, dropping", item.project_name);
                }
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

fn cmd_explain() {
    println!(
        "v1: human windows capped at 25m; agent activity 300‰; error loops 1150‰; human rework 1100‰; advisory annotations can only reduce credit."
    );
}

fn cmd_episodes(repo: &Path, transcript: &Path, session: String) -> io::Result<()> {
    let r = state::root(repo)?;
    let input = fs::read_to_string(transcript)?;
    let project = r
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("project")
        .to_string();
    let events = parse_claude_jsonl(&input, project, None::<String>, session.clone())
        .map_err(io::Error::other)?;
    record_harness_prompt_signals(&r, &input, &session)?;
    let config = hackai_scorer::FormulaConfig::default();
    let episodes = productive_episodes(&events, &config);
    let output_path = state::state_dir(&r).join("episodes.json");
    fs::create_dir_all(state::state_dir(&r))?;
    fs::write(
        &output_path,
        serde_json::to_vec_pretty(&episodes).map_err(io::Error::other)?,
    )?;
    let valid = episodes.iter().filter(|episode| episode.valid).count();
    let credited: u64 = episodes
        .iter()
        .map(|episode| episode.credited_seconds)
        .sum();
    state::audit(&r, "episodes")?;
    println!(
        "Recorded {} episode(s), {} valid, {} credited seconds. Raw transcript text was not stored.",
        episodes.len(),
        valid,
        credited
    );
    let _ = cmd_sync(&r, false);
    Ok(())
}

fn record_harness_prompt_signals(repo: &Path, transcript: &str, session: &str) -> io::Result<()> {
    let pasted_hint = matches!(
        std::env::var("HACKAI_PROMPT_PASTED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    );
    let output = state::state_dir(repo).join("prompt-events.jsonl");
    fs::create_dir_all(state::state_dir(repo))?;
    let mut events = Vec::new();
    visit_claude_prompts(transcript, |occurred_at, prompt| {
        let signals = prompt_classifier::classify_prompt(prompt);
        events.push(serde_json::json!({
            "schema_version": 1,
            "session_id": session,
            "occurred_at_unix_ms": occurred_at.0,
            "pasted_environment_hint": pasted_hint,
            "signals": signals,
        }));
    })
    .map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output)?;
    for event in events {
        serde_json::to_writer(&mut file, &event).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
    }
    Ok(())
}

fn cmd_annotate(
    repo: &Path,
    window: String,
    label: String,
    user_role: String,
    evidence: Vec<String>,
    chapter_title: Option<String>,
) -> io::Result<()> {
    let r = state::root(repo)?;
    let annotation = state::annotation_value(
        &window,
        &label,
        &user_role,
        &evidence,
        chapter_title.as_deref(),
    )?;
    state::write_annotation(&r, &annotation)?;
    println!("Advisory annotation recorded locally; it cannot raise credit.");
    Ok(())
}

fn cmd_mcp() -> io::Result<()> {
    mcp::run_mcp()
}

fn cmd_classify_prompt() -> io::Result<()> {
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt)?;
    let signals = prompt_classifier::classify_prompt(&prompt);
    println!(
        "{}",
        serde_json::to_string(&signals).map_err(io::Error::other)?
    );
    Ok(())
}

fn cmd_train_prompt_model(dataset: PathBuf, output: PathBuf) -> io::Result<()> {
    let result = prompt_model::train_jsonl(&dataset, &output)?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(io::Error::other)?
    );
    Ok(())
}

fn cmd_generate_prompt_dataset(output: PathBuf, per_level: usize) -> io::Result<()> {
    let result = prompt_model::generate_synthetic_jsonl(&output, per_level)?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(io::Error::other)?
    );
    Ok(())
}

#[derive(Serialize)]
struct PromptLabRecord {
    schema_version: u8,
    session_id: String,
    turn: u32,
    recorded_at_unix: u64,
    prompt: String,
    classification: prompt_classifier::PromptSignals,
    proficiency_label: Option<String>,
    delegation_label: Option<String>,
}

fn cmd_prompt_lab(
    log: PathBuf,
    proficiency_label: Option<String>,
    delegation_label: Option<String>,
) -> io::Result<()> {
    eprintln!("Prompt Lab — local-only log: {}", log.display());
    eprintln!("Type a prompt and press Enter for a score. Type :quit or press Ctrl-D to finish.");
    let session_id = Uuid::new_v4().to_string();
    let stdin = io::stdin();
    let mut turn = 0_u32;
    let mut prior_prompt: Option<String> = None;
    eprint!("prompt[1]> ");
    io::stderr().flush()?;
    for line in stdin.lock().lines() {
        let prompt = line?;
        if prompt.trim() == ":quit" {
            break;
        }
        record_prompt_lab_turn(
            &log,
            &session_id,
            &mut turn,
            &mut prior_prompt,
            prompt,
            proficiency_label.clone(),
            delegation_label.clone(),
        )?;
        eprint!("prompt[{}]> ", turn + 1);
        io::stderr().flush()?;
    }
    eprintln!("Prompt Lab session ended: {turn} recorded turn(s).");
    Ok(())
}

fn record_prompt_lab_turn(
    log: &Path,
    session_id: &str,
    turn: &mut u32,
    prior_prompt: &mut Option<String>,
    prompt: String,
    proficiency_label: Option<String>,
    delegation_label: Option<String>,
) -> io::Result<()> {
    if prompt.trim().is_empty() {
        eprintln!("Skipped an empty prompt.");
        return Ok(());
    }
    let used_prior_turn_context = prior_prompt.is_some();
    // Keep at most one previous short turn: enough to resolve “do that too”
    // without making this local evaluator retain an unbounded transcript.
    let context = prior_prompt
        .as_deref()
        .filter(|previous| previous.len() + prompt.len() <= 32 * 1024)
        .map(|previous| format!("{previous}\nFollow-up: {prompt}"));
    let mut classification =
        prompt_classifier::classify_prompt(context.as_deref().unwrap_or(&prompt));
    classification.used_prior_turn_context = used_prior_turn_context && context.is_some();
    let recorded_at_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let record = PromptLabRecord {
        schema_version: 1,
        session_id: session_id.to_owned(),
        turn: *turn,
        recorded_at_unix,
        prompt: prompt.clone(),
        classification,
        proficiency_label,
        delegation_label,
    };
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    serde_json::to_writer(&mut file, &record).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    println!(
        "{}",
        serde_json::to_string(&record.classification).map_err(io::Error::other)?
    );
    *prior_prompt = Some(prompt);
    *turn += 1;
    Ok(())
}

fn cmd_checkpoint_nudge(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let s = state::state_dir(&r);
    let config = state::tracker_config(&r)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
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

fn cmd_config_checkpoint(repo: &Path, value: u16) -> io::Result<()> {
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

fn cmd_merge(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());

    let token = read_stored_token(&r).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Other,
            "not authenticated; run `hackai login`",
        )
    })?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let projects = rt.block_on(api::api_get_projects(&server_url, &token))?;

    if projects.is_empty() {
        println!("No projects found on dashboard.");
        return Ok(());
    }

    println!("Your projects:");
    for (i, p) in projects.iter().enumerate() {
        let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("-");
        let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("-");
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
    let source_id = source.get("id").and_then(|v| v.as_str()).unwrap();

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
    let target_id = target.get("id").and_then(|v| v.as_str()).unwrap();

    let source_name = source.get("name").and_then(|v| v.as_str()).unwrap();
    let target_name = target.get("name").and_then(|v| v.as_str()).unwrap();

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

fn cmd_queue_list(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let queue = state::read_queue(&r);
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

fn cmd_queue_clear(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    state::write_queue(&r, &[])?;
    println!("Queue cleared.");
    Ok(())
}

fn cmd_queue_process(repo: &Path) -> io::Result<()> {
    let r = state::root(repo)?;
    let token = read_stored_token(&r)
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "not authenticated"))?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
    process_queue(&r, &token, &server_url)
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Track {
            repo,
            local_only,
            dashboard,
        } => cmd_track(&repo, local_only, dashboard)?,
        Command::Untrack {
            repo,
            local_only,
            dashboard,
        } => cmd_untrack(&repo, local_only, dashboard)?,
        Command::Status { repo } => cmd_status(&repo)?,
        Command::Audit { repo } => cmd_audit(&repo)?,
        Command::Export { preview } => cmd_export(preview)?,
        Command::Login {
            token,
            api_key,
            device,
        } => cmd_login(token, api_key, device)?,
        Command::Logout => cmd_logout()?,
        Command::Whoami => cmd_whoami()?,
        Command::Sync { repo, force } => cmd_sync(&repo, force)?,
        #[cfg(feature = "dev-tools")]
        Command::Explain => cmd_explain(),
        #[cfg(feature = "dev-tools")]
        Command::Episodes {
            repo,
            transcript,
            session,
        } => cmd_episodes(&repo, &transcript, session)?,
        Command::Annotate {
            repo,
            window,
            label,
            user_role,
            evidence,
            chapter_title,
        } => cmd_annotate(&repo, window, label, user_role, evidence, chapter_title)?,
        Command::Mcp => cmd_mcp()?,
        #[cfg(feature = "dev-tools")]
        Command::ClassifyPrompt => cmd_classify_prompt()?,
        #[cfg(feature = "dev-tools")]
        Command::TrainPromptModel { dataset, output } => cmd_train_prompt_model(dataset, output)?,
        #[cfg(feature = "dev-tools")]
        Command::GeneratePromptDataset { output, per_level } => {
            cmd_generate_prompt_dataset(output, per_level)?
        }
        #[cfg(feature = "dev-tools")]
        Command::PromptLab {
            log,
            proficiency_label,
            delegation_label,
        } => cmd_prompt_lab(log, proficiency_label, delegation_label)?,
        Command::Harness { command } => match command {
            HarnessCommand::Install { repo } => harness::install(&repo)?,
            HarnessCommand::Hook {
                repo,
                harness,
                event,
            } => harness::handle_hook(&repo, &harness, &event)?,
            HarnessCommand::Aider { repo, args } => harness::run_aider(&repo, &args)?,
        },
        Command::CheckpointNudge { repo } => cmd_checkpoint_nudge(&repo)?,
        Command::Config { command } => match command {
            ConfigCommand::CheckpointTurnInterval { repo, value } => {
                cmd_config_checkpoint(&repo, value)?
            }
        },
        Command::Merge { repo } => cmd_merge(&repo)?,
        Command::Queue { command } => {
            let repo = Path::new(".");
            match command {
                QueueCommand::List => cmd_queue_list(repo)?,
                QueueCommand::Clear => cmd_queue_clear(repo)?,
                QueueCommand::Process => cmd_queue_process(repo)?,
            }
        }
        Command::Purge { repo } | Command::Uninstall { repo } => {
            cmd_untrack(&repo, true, false)?;
        }
        Command::Install => {
            println!("MCP install not yet implemented.");
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::state::safe_terminal;
    #[test]
    fn strips_controls_and_ansi() {
        assert_eq!(safe_terminal("ok\u{1b}[31mred\u{1b}[0m\nnext"), "okrednext");
    }
}
