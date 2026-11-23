//! Native adapters for coding-agent harnesses.
//!
//! Hook payloads may contain prompts, paths, command lines, and tool output.
//! This module derives coarse local signals and persists only those signals.

use crate::{classifier, state};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const HARNESSES: [&str; 5] = ["codex", "claude-code", "opencode", "antigravity", "aider"];

#[derive(Debug, Clone, Copy)]
pub enum InstallScope {
    Project,
    Global,
}

pub fn install(repo: &Path, scope: InstallScope, policy: &str) -> io::Result<()> {
    // Configuration belongs beside the harness project even before the user
    // initializes Git. Event storage still requires a Git worktree later.
    let root = match scope {
        InstallScope::Project => repo.canonicalize()?,
        InstallScope::Global => dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?,
    };
    install_codex(&root)?;
    install_claude(&root)?;
    install_antigravity(&root)?;
    install_opencode_plugin(&root)?;
    install_mcp_configs(&root)?;
    install_skill(&root, scope, policy)?;
    println!(
        "Installed adapters and automatic HackAI MCP for Codex, Claude Code, OpenCode, and Antigravity."
    );
    println!(
        "Aider: run `hackai harness aider -- --message \"…\"` (or pass normal Aider arguments).\nAll adapters retain prompt text only in process memory and write stripped local events."
    );
    Ok(())
}

fn install_skill(root: &Path, scope: InstallScope, policy: &str) -> io::Result<()> {
    let canonical = match scope {
        InstallScope::Project => root.join("annotations/SKILL.md"),
        InstallScope::Global => root.join(".config/hackai/SKILL.md"),
    };
    if let Some(parent) = canonical.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&canonical, policy)?;

    // Keep the same server-fetched policy where each harness discovers skills.
    // Every copy is byte-identical; sync hashes the canonical project copy.
    let skill_roots = match scope {
        InstallScope::Project => vec![
            root.join(".codex/skills/hackai-annotate"),
            root.join(".claude/skills/hackai-annotate"),
            root.join(".agents/skills/hackai-annotate"),
            root.join(".gemini/skills/hackai-annotate"),
            root.join(".opencode/skills/hackai-annotate"),
        ],
        InstallScope::Global => vec![
            root.join(".codex/skills/hackai-annotate"),
            root.join(".claude/skills/hackai-annotate"),
            root.join(".agents/skills/hackai-annotate"),
            root.join(".gemini/skills/hackai-annotate"),
            root.join(".config/opencode/skills/hackai-annotate"),
        ],
    };
    for directory in skill_roots {
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("SKILL.md"), policy)?;
    }
    Ok(())
}

fn install_mcp_configs(root: &Path) -> io::Result<()> {
    merge_json(
        root.join(".mcp.json"),
        json!({"mcpServers": {"hackai": {"command": "hackai", "args": ["mcp"]}}}),
    )?;
    merge_json(
        root.join(".agents/mcp.json"),
        json!({"mcpServers": {"hackai": {"command": "hackai", "args": ["mcp"]}}}),
    )?;
    merge_json(
        root.join(".gemini/settings.json"),
        json!({"mcpServers": {"hackai": {"command": "hackai", "args": ["mcp"]}}}),
    )?;
    merge_json(
        root.join(".opencode/mcp.json"),
        json!({"mcpServers": {"hackai": {"command": "hackai", "args": ["mcp"]}}}),
    )?;
    let codex = root.join(".codex/config.toml");
    if !codex.exists() {
        if let Some(parent) = codex.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            codex,
            "[mcp_servers.hackai]\ncommand = \"hackai\"\nargs = [\"mcp\"]\n",
        )?;
    }
    Ok(())
}

pub fn handle_hook(repo: &Path, harness: &str, event: &str) -> io::Result<()> {
    if !HARNESSES.contains(&harness) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported harness",
        ));
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let payload = serde_json::from_str::<Value>(&input).unwrap_or(Value::Null);
    let prompt = find_prompt(&payload);
    let session_id = find_string(
        &payload,
        &["session_id", "sessionId", "conversationId", "thread_id"],
    )
    .unwrap_or("unknown");
    let pasted_environment_hint = matches!(
        std::env::var("HACKAI_PROMPT_PASTED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    );

    let blocked = prompt
        .map(|source| {
            let signals = classifier::classify_prompt(source);
            append_prompt_event(
                repo,
                harness,
                event,
                session_id,
                pasted_environment_hint,
                &signals,
            )?;
            Ok::<bool, io::Error>(signals.do_not_comply)
        })
        .transpose()?
        .unwrap_or(false);

    if prompt.is_none() {
        append_harness_event(repo, harness, event, session_id, &payload)?;
    }
    write_hook_response(harness, event, blocked)
}

pub fn run_aider(repo: &Path, args: &[String]) -> io::Result<()> {
    let root = state::root(repo)?;
    if let Some(prompt) = aider_message(args)? {
        let signals = classifier::classify_prompt(&prompt);
        append_prompt_event(&root, "aider", "message", "aider-cli", false, &signals)?;
        if signals.do_not_comply {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "HackAI policy: PESDO scores, credit, and annotations cannot be fabricated or manipulated.",
            ));
        }
    }
    let status = Command::new("aider")
        .args(args)
        .current_dir(root)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("aider exited with {status}")))
    }
}

fn install_codex(root: &Path) -> io::Result<()> {
    merge_json(
        root.join(".codex/hooks.json"),
        json!({"hooks": {
            "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness codex --event user-prompt", "async": true}]}],
            "PostToolUse": [{"matcher": ".*", "hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness codex --event post-tool", "async": true}]}],
            "SessionEnd": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness codex --event session-end"}]}]
        }}),
    )
}

fn install_claude(root: &Path) -> io::Result<()> {
    merge_json(
        root.join(".claude/settings.local.json"),
        json!({"hooks": {
            "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness claude-code --event user-prompt", "async": true}]}],
            "PostToolUse": [{"matcher": ".*", "hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness claude-code --event post-tool", "async": true}]}],
            "PostToolUseFailure": [{"matcher": ".*", "hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness claude-code --event tool-failure", "async": true}]}],
            "SessionEnd": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness claude-code --event session-end"}]}]
        }}),
    )
}

fn install_antigravity(root: &Path) -> io::Result<()> {
    merge_json(
        root.join(".agents/hooks.json"),
        json!({"hackai": {
            "PreInvocation": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness antigravity --event pre-invocation", "timeout": 5}]}],
            "PostToolUse": [{"matcher": ".*", "hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness antigravity --event post-tool", "timeout": 5}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "hackai harness hook --repo . --harness antigravity --event stop", "timeout": 5}]}]
        }}),
    )
}

fn install_opencode_plugin(root: &Path) -> io::Result<()> {
    let path = root.join(".opencode/plugins/hackai.ts");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // OpenCode discovers files in .opencode/plugins automatically. The plugin
    // forwards only in-memory hook payloads to this local CLI process.
    fs::write(
        path,
        r#"import { Plugin } from "@opencode/plugin"

const send = async (event: string, payload: unknown) => {
  const proc = Bun.spawn(["hackai", "harness", "hook", "--repo", ".", "--harness", "opencode", "--event", event], {
    stdin: "pipe", stdout: "ignore", stderr: "ignore",
  })
  proc.stdin.write(JSON.stringify(payload))
  proc.stdin.end()
  await proc.exited
}

export default Plugin.define({
  id: "hackai.local-telemetry",
  async setup(ctx) {
    await ctx.session.hook("prompt", (event) => send("prompt", { sessionId: event.sessionID, prompt: event.text }))
    await ctx.tool.hook("execute.before", (event) => send("tool-before", event))
    await ctx.tool.hook("execute.after", (event) => send("tool-after", event))
    await ctx.permission.hook("evaluate", (event) => send("permission", event))
    await ctx.session.hook("retry", (event) => send("retry", event))
  },
})
"#,
    )
}

fn merge_json(path: impl AsRef<Path>, additions: Value) -> io::Result<()> {
    let path = path.as_ref();
    let mut base = match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} contains invalid JSON: {error}", path.display()),
            )
        })?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error),
    };
    merge_value(&mut base, additions);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        path,
        serde_json::to_vec_pretty(&base).map_err(io::Error::other)?,
    )
}

fn merge_value(existing: &mut Value, incoming: Value) {
    match (existing, incoming) {
        (Value::Object(left), Value::Object(right)) => {
            for (key, value) in right {
                if let Some(current) = left.get_mut(&key) {
                    merge_value(current, value);
                } else {
                    left.insert(key, value);
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            for value in right {
                if !left.contains(&value) {
                    left.push(value);
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

fn find_prompt(value: &Value) -> Option<&str> {
    for key in ["prompt", "lastUserInput", "text"] {
        if let Some(text) = value.get(key).and_then(Value::as_str) {
            return Some(text);
        }
    }
    None
}

fn find_string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn now_ms() -> io::Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis())
}

fn append_prompt_event(
    repo: &Path,
    harness: &str,
    event: &str,
    session_id: &str,
    pasted_environment_hint: bool,
    signals: &classifier::PromptSignals,
) -> io::Result<()> {
    append_line(
        repo,
        "prompt-events.jsonl",
        &json!({
            "schema_version": 1, "harness": harness, "event": event,
            "session_id": session_id, "occurred_at_unix_ms": now_ms()?,
            "pasted_environment_hint": pasted_environment_hint, "signals": signals,
        }),
    )
}

fn append_harness_event(
    repo: &Path,
    harness: &str,
    event: &str,
    session_id: &str,
    payload: &Value,
) -> io::Result<()> {
    let tool = find_string(payload, &["tool_name", "tool", "name"]).unwrap_or("unknown");
    let outcome = if payload.get("error").is_some() {
        "error"
    } else {
        "observed"
    };
    append_line(
        repo,
        "harness-events.jsonl",
        &json!({
            "schema_version": 1, "harness": harness, "event": event,
            "session_id": session_id, "occurred_at_unix_ms": now_ms()?,
            "tool_category": tool_category(tool), "outcome": outcome,
        }),
    )
}

fn append_line(repo: &Path, file_name: &str, value: &Value) -> io::Result<()> {
    let directory = state::state_dir(repo);
    fs::create_dir_all(&directory)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(file_name))?;
    serde_json::to_writer(&mut file, value).map_err(io::Error::other)?;
    file.write_all(b"\n")
}

fn tool_category(name: &str) -> &'static str {
    let value = name.to_ascii_lowercase();
    if value.contains("test") {
        "test"
    } else if value.contains("lint") {
        "lint"
    } else if value.contains("build") {
        "build"
    } else if value.contains("write") || value.contains("edit") || value.contains("replace") {
        "edit"
    } else if value.contains("read") || value.contains("view") {
        "read"
    } else if value.contains("subagent") || value.contains("task") {
        "subagent"
    } else {
        "other"
    }
}

fn write_hook_response(harness: &str, event: &str, blocked: bool) -> io::Result<()> {
    let response = if !blocked {
        json!({})
    } else if harness == "antigravity" && event == "pre-invocation" {
        json!({"injectSteps": [{"errorMessage": {"userMessage": "HackAI policy: PESDO scores and credit cannot be manipulated."}}], "terminationBehavior": "terminate"})
    } else {
        json!({"decision": "block", "reason": "HackAI policy: PESDO scores, credit, and annotations cannot be fabricated or manipulated."})
    };
    println!(
        "{}",
        serde_json::to_string(&response).map_err(io::Error::other)?
    );
    Ok(())
}

fn aider_message(args: &[String]) -> io::Result<Option<String>> {
    for (index, arg) in args.iter().enumerate() {
        if matches!(arg.as_str(), "--message" | "--msg" | "-m") {
            return Ok(args.get(index + 1).cloned());
        }
        if matches!(arg.as_str(), "--message-file" | "-f") {
            return args.get(index + 1).map(fs::read_to_string).transpose();
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_existing_config_and_adds_hooks() {
        let mut existing = json!({"hooks": {"Stop": [{"command": "keep"}]}});
        merge_value(
            &mut existing,
            json!({"hooks": {"Stop": [{"command": "new"}]}}),
        );
        assert_eq!(existing["hooks"]["Stop"].as_array().map(Vec::len), Some(2));
    }
    #[test]
    fn only_extracts_known_prompt_fields() {
        assert_eq!(find_prompt(&json!({"prompt":"hello"})), Some("hello"));
        assert_eq!(find_prompt(&json!({"tool_input":"secret"})), None);
    }
}
