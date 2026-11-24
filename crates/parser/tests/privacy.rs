use hackai_parser::{ParseError, parse_claude_jsonl};

#[test]
fn agent_git_commit_uploads_no_command_or_sha() {
    let events = parse_claude_jsonl(
        r#"{"type":"assistant","timestamp":1,"message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{"command":"git commit -m PRIVATE_COMMIT"}}]}}"#,
        (),
        None::<()>,
        "fixture".into(),
    )
    .unwrap_or_else(|_| unreachable!());
    let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
    assert!(
        encoded.contains("\"kind\":\"GitCommit\""),
        "a git commit event should be emitted"
    );
    assert!(encoded.contains("\"role\":\"Agent\""));
    // No commit SHA, command, or prompt text may be stored.
    assert!(!encoded.contains("\"sha\":"));
    assert!(!encoded.contains("git commit"));
    assert!(!encoded.contains("PRIVATE_COMMIT"));
}

#[test]
fn fixture_text_is_not_serialized() {
    let events = parse_claude_jsonl(
        include_str!("fixtures/privacy.jsonl"),
        (),
        None::<()>,
        "fixture".into(),
    )
    .unwrap_or_else(|_| unreachable!());
    let encoded = serde_json::to_string(&events).unwrap_or_else(|_| unreachable!());
    assert!(!encoded.contains("PRIVATE_PROMPT_DO_NOT_UPLOAD"));
    assert!(!encoded.contains("PRIVATE_TOOL_RESULT_DO_NOT_UPLOAD"));
}

#[test]
fn malformed_fixture_is_rejected() {
    assert!(matches!(
        parse_claude_jsonl(
            include_str!("fixtures/malformed.jsonl"),
            (),
            None::<()>,
            "fixture".into()
        ),
        Err(ParseError::Json { line: 3, .. })
    ));
}
