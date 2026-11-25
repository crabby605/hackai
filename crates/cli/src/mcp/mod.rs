// MCP stdio server
use crate::state::{write_annotation, write_user_feedback};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::path::Path;

fn mcp_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn mcp_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn run_mcp() -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                writeln!(
                    stdout,
                    "{}",
                    mcp_error(Value::Null, -32700, "invalid JSON-RPC request")
                )?;
                stdout.flush()?;
                continue;
            }
        };

        let Some(id) = request.get("id").cloned() else {
            continue;
        };

        let response = match request.get("method").and_then(Value::as_str) {
            Some("initialize") => {
                let client_version = request
                    .get("params")
                    .and_then(|p| p.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .unwrap_or("2024-11-05");
                mcp_response(
                    id,
                    json!({
                        "protocolVersion": client_version,
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "hackai",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }),
                )
            }
            Some("ping") => mcp_response(id, json!({})),
            Some("tools/list") => mcp_response(
                id,
                json!({
                    "tools": [
                        {
                            "name": "annotate",
                            "description": "Record a local-only, evidence-backed advisory struggle annotation. It never raises credit.",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["window", "label", "user_role", "evidence"],
                                "properties": {
                                    "window": { "type": "string", "maxLength": 128 },
                                    "label": { "enum": ["debugging", "guided", "delegated", "reading", "exploring"] },
                                    "user_role": { "enum": ["drove", "steered", "approved", "absent"] },
                                    "evidence": { "type": "array", "minItems": 1, "maxItems": 16, "items": { "type": "string", "maxLength": 128 } },
                                    "chapter_title": { "type": "string", "maxLength": 120 }
                                }
                            }
                        },
                        {
                            "name": "get_pesdo_score",
                            "description": "Get PESDO score (weighted credit / human-present time) for the current project.",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "project": { "type": "string" }
                                }
                            }
                        },
                        {
                            "name": "record_user_feedback",
                            "description": "Record an opt-in, constructive coaching note of at most 50 words. It is uploaded by the local background sync; do not include prompt text, code, paths, secrets, or quotes.",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["feedback"],
                                "properties": {
                                    "feedback": { "type": "string", "minLength": 1, "maxLength": 600 }
                                }
                            }
                        },
                        {
                            "name": "get_formula_config",
                            "description": "Get the current scoring formula configuration (version, weights, caps).",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {}
                            }
                        },
                        {
                            "name": "get_episode_summary",
                            "description": "Get episode metrics for the current project (count, credited time, resolution rate).",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "project": { "type": "string" }
                                }
                            }
                        },
                        {
                            "name": "get_user_stats",
                            "description": "Get aggregate stats for the authenticated user across all projects.",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {}
                            }
                        },
                        {
                            "name": "classify_prompt",
                            "description": "Classify prompt tone and detect score-manipulation attempts locally. The prompt is never stored, synced, or used for PESDO/credit; do_not_comply means the request must be refused.",
                            "inputSchema": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["prompt"],
                                "properties": {
                                    "prompt": { "type": "string", "maxLength": 32768 }
                                }
                            }
                        }
                    ]
                }),
            ),
            Some("tools/call") => {
                let params = request.get("params").and_then(Value::as_object);
                let tool_name = params.and_then(|p| p.get("name")).and_then(Value::as_str);
                let arguments = params
                    .and_then(|p| p.get("arguments"))
                    .and_then(Value::as_object);

                let Some(args) = arguments else {
                    writeln!(stdout, "{}", mcp_error(id, -32602, "missing arguments"))?;
                    stdout.flush()?;
                    continue;
                };

                match tool_name {
                    Some("record_user_feedback") => {
                        let feedback = match args.get("feedback").and_then(Value::as_str) {
                            Some(value) => value,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "missing required parameter 'feedback'" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let repo = match crate::state::root(Path::new(".")) {
                            Ok(root) => root,
                            Err(error) => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": format!("Feedback rejected: {error}") }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        match write_user_feedback(&repo, feedback) {
                            Ok(()) => mcp_response(
                                id,
                                json!({"content": [{"type": "text", "text": "Feedback recorded locally and will be uploaded by the next background sync."}]}),
                            ),
                            Err(error) => mcp_response(
                                id,
                                json!({"content": [{"type": "text", "text": format!("Feedback rejected: {error}")}], "isError": true}),
                            ),
                        }
                    }
                    Some("annotate") => {
                        let window = match args.get("window").and_then(Value::as_str) {
                            Some(w) => w,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "missing required parameter 'window'" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let label = match args.get("label").and_then(Value::as_str) {
                            Some(l) => l,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "missing required parameter 'label'" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let user_role = match args.get("user_role").and_then(Value::as_str) {
                            Some(r) => r,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "missing required parameter 'user_role'" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let evidence_arr = match args.get("evidence").and_then(Value::as_array) {
                            Some(arr) => arr,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "missing required parameter 'evidence'" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let evidence = match evidence_arr
                            .iter()
                            .map(|v| v.as_str().map(str::to_owned))
                            .collect::<Option<Vec<String>>>()
                        {
                            Some(ev) => ev,
                            None => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "'evidence' items must be strings" }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };
                        let title = args.get("chapter_title").and_then(Value::as_str);

                        let repo = match crate::state::root(Path::new(".")) {
                            Ok(r) => r,
                            Err(_) => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": "Annotation rejected: current working directory is not inside a git repository." }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };

                        let annotation = match crate::state::annotation_value(
                            window, label, user_role, &evidence, title,
                        ) {
                            Ok(a) => a,
                            Err(e) => {
                                let res = json!({
                                    "content": [{ "type": "text", "text": format!("Annotation rejected: {e}") }],
                                    "isError": true
                                });
                                writeln!(stdout, "{}", mcp_response(id, res))?;
                                stdout.flush()?;
                                continue;
                            }
                        };

                        match write_annotation(&repo, &annotation) {
                            Ok(()) => mcp_response(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": "Advisory annotation recorded locally. It cannot raise credit." }]
                                }),
                            ),
                            Err(e) => mcp_response(
                                id,
                                json!({
                                    "content": [{ "type": "text", "text": format!("Failed to record annotation: {e}") }],
                                    "isError": true
                                }),
                            ),
                        }
                    }
                    Some("get_pesdo_score") => {
                        let project = args.get("project").and_then(Value::as_str).unwrap_or("");
                        mcp_response(
                            id,
                            json!({ "content": [{ "type": "text", "text": format!("PESDO score for {}: requires authenticated API call", project) }] }),
                        )
                    }
                    Some("get_formula_config") => {
                        let config = hackai_scorer::FormulaConfig::default();
                        mcp_response(
                            id,
                            json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&config).unwrap_or_default() }] }),
                        )
                    }
                    Some("get_episode_summary") => {
                        let project = args.get("project").and_then(Value::as_str).unwrap_or("");
                        mcp_response(
                            id,
                            json!({ "content": [{ "type": "text", "text": format!("Episode summary for {}: requires local episode data", project) }] }),
                        )
                    }
                    Some("get_user_stats") => mcp_response(
                        id,
                        json!({ "content": [{ "type": "text", "text": "User stats: requires authenticated API call" }] }),
                    ),
                    Some("classify_prompt") => {
                        let Some(prompt) = args.get("prompt").and_then(Value::as_str) else {
                            let res = json!({
                                "content": [{ "type": "text", "text": "missing required parameter 'prompt'" }],
                                "isError": true
                            });
                            writeln!(stdout, "{}", mcp_response(id, res))?;
                            stdout.flush()?;
                            continue;
                        };
                        match serde_json::to_string(&crate::classifier::classify_prompt(prompt)) {
                            Ok(signals) => mcp_response(
                                id,
                                json!({ "content": [{ "type": "text", "text": signals }] }),
                            ),
                            Err(error) => {
                                mcp_error(id, -32603, &format!("classification failed: {error}"))
                            }
                        }
                    }
                    Some(_) => mcp_error(id, -32601, "method not found"),
                    None => mcp_error(id, -32600, "missing method"),
                }
            }
            Some(_) => mcp_error(id, -32601, "method not found"),
            None => mcp_error(id, -32600, "missing method"),
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
