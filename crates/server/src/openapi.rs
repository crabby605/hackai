#![forbid(unsafe_code)]

//! Serves the static OpenAPI document and a Scalar API reference UI.
//!
//! Deliberately dependency-light: no OpenAPI codegen framework (utoipa/schemars).
//! The spec is a hand-written `openapi.json` kept accurate to the routes in
//! `main.rs` and the route modules. Scalar is loaded from CDN and points at the
//! `/api/openapi.json` endpoint.

use axum::{
    Router,
    http::{HeaderMap, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};

use crate::AppState;

/// OpenAPI document, embedded at compile time from `openapi.json`.
const OPENAPI_JSON: &str = include_str!("../openapi.json");

/// Scalar reference HTML, loaded from CDN.
const SCALAR_HTML: &str = r#"<!doctype html>
<html>
  <head>
    <title>HackAI API Reference</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
  </head>
  <body>
    <script>
      document.addEventListener('DOMContentLoaded', () => {
        Scalar.createApiReferenceElement({
          url: '/api/openapi.json',
          hideDownloadButton: true,
          hideSearch: false,
          hideModelToggle: false,
          hideTestRequestButton: false,
          layout: 'modern',
          defaultHttpClient: { targetKey: 'javascript', clientKey: 'fetch' },
          theme: 'solarized',
          metaData: {
            display: { 'document-title': 'HackAI API Reference' }
          }
        }).mount(document.body);
      });
    </script>
  </body>
</html>"#;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/openapi.json", get(serve_openapi))
        .route("/docs", get(serve_scalar))
}

/// Raw OpenAPI JSON document.
async fn serve_openapi(_headers: HeaderMap) -> Response {
    (
        [(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json; charset=utf-8"),
        )],
        OPENAPI_JSON,
    )
        .into_response()
}

/// Scalar API reference HTML UI.
async fn serve_scalar(_headers: HeaderMap) -> Html<&'static str> {
    Html(SCALAR_HTML)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_json_is_valid_json() {
        let result = serde_json::from_str::<serde_json::Value>(OPENAPI_JSON);
        assert!(result.is_ok(), "openapi.json must parse as JSON");
        if let Ok(document) = result {
            assert_eq!(document["openapi"], "3.0.3");
        }
    }

    #[test]
    fn openapi_json_declares_security_schemes() {
        assert!(OPENAPI_JSON.contains("\"SessionCookie\""));
        assert!(OPENAPI_JSON.contains("\"BearerToken\""));
    }

    #[test]
    fn api_key_route_is_declared_once_with_both_methods() {
        assert_eq!(OPENAPI_JSON.matches("\"/api/v1/me/api-key\"").count(), 1);
        assert!(OPENAPI_JSON.contains("\"has_key\""));
        assert!(OPENAPI_JSON.contains("\"api_key\""));
    }

    #[test]
    fn scalar_html_references_openapi_endpoint() {
        assert!(SCALAR_HTML.contains("/api/openapi.json"));
    }
}
