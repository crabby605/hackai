//! Local-first AI coding time tracker
//!
//! Modular structure:
//! - commands: CLI command implementations grouped by domain
//! - classifier: Local prompt classification (ML + rules)
//! - installer: Hook/integration installation
//! - state: local paths, config, queue, audit, checkpoint
//! - api: HTTP client functions for server communication
//! - git: git command wrappers
//! - harness: harness-specific hook handlers
//! - mcp: MCP server for advisory annotations
//! - processor: deterministic dialogue processor
//! - evidence_export: privacy-safe evidence bundle construction

mod api;
mod classifier;
mod commands;
mod evidence_export;
mod git;
mod harness;
mod installer;
mod mcp;
mod processor;
mod state;

use clap::{Parser, Subcommand, ValueEnum};
use keyring::Entry;
use std::{
    io,
    path::{Path, PathBuf},
};

fn tokio_rt() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| io::Error::other(format!("tokio runtime: {e}")))
}

const KEYRING_SERVICE: &str = "hackai";
const KEYRING_USER: &str = "api_token";

fn token_entry(root: &Path) -> io::Result<Entry> {
    let repo_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default");
    Entry::new(KEYRING_SERVICE, &format!("{KEYRING_USER}_{repo_name}"))
        .map_err(|e| io::Error::other(format!("keyring entry error: {e}")))
}

fn read_stored_token(root: &Path) -> Option<String> {
    token_entry(root)
        .ok()?
        .get_password()
        .ok()
        .and_then(|t| (!t.trim().is_empty()).then_some(t))
        .or_else(|| api::read_local_config(root).ok().and_then(|c| c.api_key))
}

fn write_stored_token(root: &Path, token: &str) -> io::Result<()> {
    token_entry(root)?
        .set_password(token)
        .map_err(io::Error::other)
}

fn clear_stored_token(root: &Path) -> io::Result<()> {
    match token_entry(root)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(io::Error::other(error)),
    }
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
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Print the redacted bundle summary before any upload.
        #[arg(long)]
        preview: bool,
        /// Upload the redacted bundle after printing its preview. Requires --preview.
        #[arg(long, requires = "preview")]
        upload: bool,
    },
    /// Ingest a local harness transcript into the privacy-preserving event store.
    Ingest {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(value_enum)]
        harness: TranscriptHarness,
        /// JSONL/Markdown transcript, or OpenCode's opencode.db.
        source: PathBuf,
        #[arg(long, default_value = "default")]
        session: String,
        /// Required for Aider because its default Markdown history lacks turn timestamps.
        #[arg(long)]
        session_started_ms: Option<i64>,
    },
    /// Continuously ingest one local harness transcript; duplicate events are ignored.
    Watch {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(value_enum)]
        harness: TranscriptHarness,
        source: PathBuf,
        #[arg(long, default_value = "default")]
        session: String,
        #[arg(long)]
        session_started_ms: Option<i64>,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=300))]
        interval_seconds: u64,
    },
    /// Discover and ingest transcripts that can be safely associated with this repository.
    Collect {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    /// Collect derived git evidence once, without reading or storing paths, code, or commit text.
    Observe {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Include commits at or after this Unix timestamp in milliseconds.
        #[arg(long)]
        since_ms: Option<i64>,
        /// Poll derived worktree state at this interval. Omit for one collection pass.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=300))]
        interval_seconds: Option<u64>,
    },
    /// Recompute privacy-safe dialogue and episode summaries from local derived events.
    Process {
        #[arg(default_value = ".")]
        repo: PathBuf,
    },
    Sync {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Force sync even if queue is empty
        #[arg(long)]
        force: bool,
        /// Run the local collection and upload worker every five minutes.
        #[arg(long, hide = true)]
        background: bool,
        /// Install the five-minute background worker for this project.
        #[arg(long)]
        install_background: bool,
    },
    /// Call an authenticated server endpoint when a harness has no native tool.
    Api {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value = "GET")]
        method: String,
        /// Must be /api/v1/... (or the shorthand /v1/...).
        #[arg(long)]
        path: String,
        /// JSON request body, used for POST/PUT/PATCH requests.
        #[arg(long)]
        body: Option<String>,
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
    /// Set up MCP for all Coding IDEs and store a nonce.
    Install {
        #[arg(default_value = ".")]
        repo: PathBuf,
        /// Install adapters and the policy into the user profile instead of this project.
        #[arg(long)]
        global: bool,
    },
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
    /// Check for and install the latest hackai release (release installs only)
    Update {
        /// Skip confirmation prompt and install the update immediately
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum TranscriptHarness {
    Claude,
    Codex,
    Opencode,
    Gemini,
    Aider,
    QwenCode,
    KimiCode,
    ZCode,
    Copilot,
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
    crate::commands::track::cmd_track(repo, local_only, dashboard)
}

fn cmd_untrack(repo: &Path, local_only: bool, dashboard: bool) -> io::Result<()> {
    crate::commands::track::cmd_untrack(repo, local_only, dashboard)
}

fn cmd_status(repo: &Path) -> io::Result<()> {
    crate::commands::track::cmd_status(repo)
}

fn cmd_audit(repo: &Path) -> io::Result<()> {
    crate::commands::track::cmd_audit(repo)
}

fn cmd_export(repo: &Path, preview: bool, upload: bool) -> io::Result<()> {
    crate::commands::export::cmd_export(repo, preview, upload)
}

fn cmd_login(token: Option<String>, api_key: Option<String>, device: bool) -> io::Result<()> {
    crate::commands::auth::cmd_login(token, api_key, device)
}

fn cmd_logout() -> io::Result<()> {
    crate::commands::auth::cmd_logout()
}

fn cmd_whoami() -> io::Result<()> {
    crate::commands::auth::cmd_whoami()
}

fn cmd_sync(
    repo: &Path,
    force: bool,
    background: bool,
    install_background: bool,
) -> io::Result<()> {
    crate::commands::sync::cmd_sync(repo, force, background, install_background)
}

fn cmd_api(repo: &Path, method: String, path: String, body: Option<String>) -> io::Result<()> {
    crate::commands::sync::cmd_api(repo, method, path, body)
}

fn cmd_ingest(
    repo: &Path,
    harness: TranscriptHarness,
    source: &Path,
    session: String,
    session_started_ms: Option<i64>,
) -> io::Result<()> {
    crate::commands::ingest::cmd_ingest(repo, harness, source, session, session_started_ms)
}

fn cmd_watch(
    repo: &Path,
    harness: TranscriptHarness,
    source: &Path,
    session: String,
    session_started_ms: Option<i64>,
    interval_seconds: u64,
) -> io::Result<()> {
    crate::commands::ingest::cmd_watch(
        repo,
        harness,
        source,
        session,
        session_started_ms,
        interval_seconds,
    )
}

fn cmd_collect(repo: &Path) -> io::Result<()> {
    crate::commands::collect::cmd_collect(repo)
}

fn cmd_observe(
    repo: &Path,
    since_ms: Option<i64>,
    interval_seconds: Option<u64>,
) -> io::Result<()> {
    crate::commands::collect::cmd_observe(repo, since_ms, interval_seconds)
}

fn cmd_process(repo: &Path) -> io::Result<()> {
    let root = state::root(repo)?;
    let output = processor::process_and_write(&root)?;
    println!(
        "Derived {} dialogue turn(s) across {} episode(s). Summary: {}",
        output.dialogue_turns.len(),
        output.episodes.len(),
        state::state_dir(&root)
            .join("dialogue-summary.json")
            .display(),
    );
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
    let _ = (repo, window, label, user_role, evidence, chapter_title);
    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "annotations are agent-only; use the HackAI MCP annotate tool from the coding harness",
    ))
}

fn cmd_mcp() -> io::Result<()> {
    mcp::run_mcp()
}

fn cmd_checkpoint_nudge(repo: &Path) -> io::Result<()> {
    crate::installer::run_checkpoint_nudge(repo)
}

fn cmd_config_checkpoint(repo: &Path, value: u16) -> io::Result<()> {
    crate::installer::set_checkpoint_interval(repo, value)
}

fn cmd_merge(repo: &Path) -> io::Result<()> {
    crate::commands::merge::cmd_merge(repo)
}

fn cmd_queue_list(repo: &Path) -> io::Result<()> {
    crate::commands::queue::cmd_queue_list(repo)
}

fn cmd_queue_clear(repo: &Path) -> io::Result<()> {
    crate::commands::queue::cmd_queue_clear(repo)
}

fn cmd_queue_process(repo: &Path) -> io::Result<()> {
    crate::commands::queue::cmd_queue_process(repo)
}

fn cmd_install(repo: &Path, global: bool) -> io::Result<()> {
    let r = state::root(repo)?;
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());
    let policy = api::fetch_annotation_policy(&server_url)
        .map_err(|error| io::Error::other(format!("could not fetch the server policy: {error}")))?;
    crate::harness::install(
        &r,
        if global {
            crate::harness::InstallScope::Global
        } else {
            crate::harness::InstallScope::Project
        },
        &policy,
    )?;
    Ok(())
}

fn cmd_purge(repo: &Path) -> io::Result<()> {
    crate::commands::track::cmd_untrack(repo, true, false)
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    // Automatic update check for interactive user-facing invocations only.
    // Nonfatal and gated to once per 24 hours.
    if !matches!(
        &cli.command,
        Command::Update { .. }
            | Command::Mcp
            | Command::Harness {
                command: HarnessCommand::Hook { .. }
            }
            | Command::CheckpointNudge { .. }
    ) {
        let _ = crate::commands::update::background_check();
    }

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
        Command::Export {
            repo,
            preview,
            upload,
        } => cmd_export(&repo, preview, upload)?,
        Command::Ingest {
            repo,
            harness,
            source,
            session,
            session_started_ms,
        } => cmd_ingest(&repo, harness, &source, session, session_started_ms)?,
        Command::Watch {
            repo,
            harness,
            source,
            session,
            session_started_ms,
            interval_seconds,
        } => cmd_watch(
            &repo,
            harness,
            &source,
            session,
            session_started_ms,
            interval_seconds,
        )?,
        Command::Collect { repo } => cmd_collect(&repo)?,
        Command::Observe {
            repo,
            since_ms,
            interval_seconds,
        } => cmd_observe(&repo, since_ms, interval_seconds)?,
        Command::Process { repo } => cmd_process(&repo)?,
        Command::Login {
            token,
            api_key,
            device,
        } => cmd_login(token, api_key, device)?,
        Command::Logout => cmd_logout()?,
        Command::Whoami => cmd_whoami()?,
        Command::Sync {
            repo,
            force,
            background,
            install_background,
        } => cmd_sync(&repo, force, background, install_background)?,
        Command::Api {
            repo,
            method,
            path,
            body,
        } => cmd_api(&repo, method, path, body)?,
        Command::Annotate {
            repo,
            window,
            label,
            user_role,
            evidence,
            chapter_title,
        } => cmd_annotate(&repo, window, label, user_role, evidence, chapter_title)?,
        Command::Mcp => cmd_mcp()?,
        Command::Harness { command } => match command {
            HarnessCommand::Install { repo } => crate::harness::install(
                &repo,
                crate::harness::InstallScope::Project,
                &crate::api::fetch_annotation_policy(
                    &std::env::var("HACKAI_SERVER_URL")
                        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into()),
                )?,
            )?,
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
        Command::Purge { repo } | Command::Uninstall { repo } => cmd_purge(&repo)?,
        Command::Install { repo, global } => cmd_install(&repo, global)?,
        Command::Update { yes } => crate::commands::update::cmd_update(yes)?,
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
