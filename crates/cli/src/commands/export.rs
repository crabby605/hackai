use crate::{api, evidence_export, read_stored_token, state};
use std::{io, path::Path};

pub fn cmd_export(repo: &Path, preview: bool, upload: bool) -> io::Result<()> {
    if !preview {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "export requires --preview; it never uploads without showing its redacted summary",
        ));
    }
    let root = state::root(repo)?;
    let bundle = evidence_export::build(&root)?;
    println!("Redacted evidence-bundle preview");
    println!(
        "  bundle: {}",
        bundle.value["bundle_id"].as_str().unwrap_or("unknown")
    );
    println!("  events: {}", bundle.event_count);
    println!(
        "  processor: {}",
        bundle.value["processor_version"]
            .as_str()
            .unwrap_or("unknown")
    );
    println!(
        "  sources: {}",
        bundle
            .source_counts
            .iter()
            .map(|(source, count)| format!("{source}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  contains: derived timestamps, hashed identifiers, event kinds, buckets, and reason codes"
    );
    println!("  excludes: prompt/reply text, code, paths, tool output, and annotations");
    if upload {
        let token = read_stored_token(&root)
            .ok_or_else(|| io::Error::other("not authenticated; run `hackai login` first"))?;
        let config = api::read_local_config(&root)?;
        let server_url = std::env::var("HACKAI_SERVER_URL")
            .ok()
            .or(config.server_url)
            .unwrap_or_else(|| "https://hackai.crabby605.hackclub.app".to_string());
        let response = api::api_request(
            &server_url,
            &token,
            "POST",
            "/api/v1/evidence-bundles",
            Some(bundle.value),
        )?;
        println!("Uploaded redacted evidence bundle: {}", response);
    } else {
        println!("Preview only. Re-run with --preview --upload to opt in to upload.");
    }
    state::audit(
        &root,
        if upload {
            "export-upload"
        } else {
            "export-preview"
        },
    )?;
    Ok(())
}