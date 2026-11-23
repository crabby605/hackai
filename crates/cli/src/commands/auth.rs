use crate::{api, clear_stored_token, read_stored_token, state, tokio_rt, write_stored_token};
use std::io::Write;
use std::{io, path::Path, sync::mpsc, thread, time::Duration};

pub fn cmd_login(token: Option<String>, api_key: Option<String>, _device: bool) -> io::Result<()> {
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
    let rt = tokio_rt()?;
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
    let (status_tx, status_rx) = mpsc::channel::<io::Result<serde_json::Value>>();
    let poll_server = server_url.clone();
    let poll_device = device_code.to_string();
    thread::spawn(move || {
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
            thread::sleep(Duration::from_secs(2));
        }
    });
    let (input_tx, input_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
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
        let status = match status_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(io::Error::other("authorization polling stopped"));
            }
        };
        let status_str = status
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending");
        match status_str {
            "approved" => {
                let token = status
                    .get("token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| io::Error::other("pairing response missing token"))?;
                write_stored_token(&r, token)?;
                println!("\n✓ Authorized! API token stored in system keyring.");
                println!("Run `hackai sync` to sync your data.");
                return Ok(());
            }
            "pending" => {
                print!(".");
                io::stdout().flush().ok();
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

pub fn cmd_logout() -> io::Result<()> {
    let r = state::root(Path::new("."))?;
    clear_stored_token(&r)?;
    println!("Logged out. Local API token removed from keyring.");
    Ok(())
}

pub fn cmd_whoami() -> io::Result<()> {
    let r = state::root(Path::new("."))?;
    let token = read_stored_token(&r);

    if token.is_none() {
        println!("Not authenticated. Run `hackai login` to authenticate.");
        return Ok(());
    }

    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".to_string());

    let rt = tokio_rt()?;
    let token_ref = token.as_ref().ok_or_else(|| io::Error::other("no token"))?;
    let user = rt.block_on(api::api_get_me(&server_url, token_ref))?;

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
