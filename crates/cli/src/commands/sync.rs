use crate::commands::queue_process::process_queue;
use crate::{api, evidence_export, read_stored_token, state};
use sha2::Digest;
use std::{env, fs, io, path::Path, process::Command, time::Duration};

const BACKGROUND_INTERVAL_SECONDS: u64 = 300;

pub fn cmd_sync(
    repo: &Path,
    force: bool,
    background: bool,
    install_background: bool,
) -> io::Result<()> {
    let r = state::root(repo)?;
    if install_background {
        return install_background_worker(&r);
    }
    if background {
        loop {
            let _ = crate::commands::collect::cmd_collect(&r);
            let _ = crate::processor::process_and_write(&r);
            if state::server_upload_authorized(&r) {
                let token = read_stored_token(&r);
                if let Some(ref token) = token {
                    let _ = process_queue(
                        &r,
                        token,
                        &std::env::var("HACKAI_SERVER_URL")
                            .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into()),
                    );
                }
                if let Err(error) = sync_once(&r, force) {
                    eprintln!("Background sync failed: {error}");
                }
            }
            std::thread::sleep(Duration::from_secs(BACKGROUND_INTERVAL_SECONDS));
        }
    }
    sync_once(&r, force)
}

fn sync_once(r: &Path, force: bool) -> io::Result<()> {
    if !state::server_upload_consented(r) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "server upload is disabled for this project; run `hackai track --dashboard` to opt in",
        ));
    }
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".into());

    let token = read_stored_token(r);

    let payload = api::build_sync_payload(r)?;

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
                state::audit(r, "sync")?;

                // Dashboard tracking is an explicit opt-in.  Send the
                // derived-only dialogue trace alongside normal sync data so
                // the dashboard can show what happened in a session.  A
                // missing local event store is normal for a newly tracked
                // project and must not make the main sync fail.
                match evidence_export::build(r) {
                    Ok(bundle) => {
                        match api::upload_evidence_bundle(&server_url, token, bundle.value) {
                            Ok(response) => println!(
                                "Synced redacted dialogue trace · {} event(s)",
                                response
                                    .get("events_stored")
                                    .and_then(|value| value.as_u64())
                                    .unwrap_or(0)
                            ),
                            Err(error) => {
                                eprintln!("Derived dialogue trace sync deferred: {error}")
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => eprintln!("Could not build derived dialogue trace: {error}"),
                }

                let queue = state::read_queue(r)?;
                if !queue.is_empty() {
                    println!("Processing {} queued sync(s)...", queue.len());
                    process_queue(r, token, &server_url)?;
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!("Sync failed: {e}");
                state::enqueue_sync(r, r, payload.clone())?;
                println!("Sync queued locally (will retry on next successful sync).");
                return Ok(());
            }
        }
    }

    if force || token.is_none() {
        state::enqueue_sync(r, r, payload)?;
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

pub fn install_background_worker(repo: &Path) -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "automatic background sync currently requires macOS",
        ));
    }
    let home = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?;
    let executable = env::current_exe()?;
    let digest = sha2::Sha256::digest(repo.to_string_lossy().as_bytes());
    let label = format!(
        "app.hackclub.hackai.{}",
        format!("{digest:x}")[..16].to_owned()
    );
    let directory = home.join("Library/LaunchAgents");
    fs::create_dir_all(&directory)?;
    let plist = directory.join(format!("{label}.plist"));
    let logs = state::state_dir(repo);
    fs::create_dir_all(&logs)?;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>{}</string><string>sync</string><string>{}</string><string>--background</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>\n",
        xml_escape(&executable.to_string_lossy()),
        xml_escape(&repo.to_string_lossy()),
        xml_escape(&logs.join("background-sync.log").to_string_lossy()),
        xml_escape(&logs.join("background-sync-error.log").to_string_lossy()),
    );
    fs::write(&plist, xml)?;
    let uid = current_uid()?;
    let target = format!("gui/{uid}");
    let _ = Command::new("launchctl")
        .args(["bootout", &target, &plist.to_string_lossy()])
        .status();
    let status = Command::new("launchctl")
        .args(["bootstrap", &target, &plist.to_string_lossy()])
        .status()?;
    if !status.success() {
        return Err(io::Error::other(
            "launchctl could not install the background sync worker",
        ));
    }
    println!(
        "Background sync installed: collection and upload run every {BACKGROUND_INTERVAL_SECONDS} seconds."
    );
    Ok(())
}

fn current_uid() -> io::Result<String> {
    let output = Command::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(io::Error::other("could not determine current user ID"));
    }
    String::from_utf8(output.stdout)
        .map_err(io::Error::other)
        .map(|value| value.trim().to_owned())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
