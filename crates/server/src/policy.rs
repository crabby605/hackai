#![forbid(unsafe_code)]

//! Public, versioned annotation policy served to CLI/harness installers.

use axum::{http::header, response::Response, routing::get};
use sha2::{Digest, Sha256};

use crate::AppState;

const SKILL: &str = include_str!("../../../annotations/SKILL.md");

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/policy/skill", get(skill))
}

async fn skill() -> Response<String> {
    let sha256 = format!("{:x}", Sha256::digest(SKILL.as_bytes()));
    let mut response = Response::new(SKILL.to_owned());
    if let Ok(value) = "text/markdown; charset=utf-8".parse() {
        response.headers_mut().insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = sha256.parse() {
        response
            .headers_mut()
            .insert("X-HackAI-Policy-SHA256", value);
    }
    response
}
