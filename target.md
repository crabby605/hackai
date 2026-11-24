HackAI / PESDO: Design Handoff
Compiled from the design discussion on 2026-11-22. Intended as the single source of truth for implementing agents.
Status tags. DECIDED = the owner (Vihaan) said it. PROPOSED = suggested by Claude, not confirmed. OPEN = unanswered question. UNVERIFIED = factual claim to check against current docs or real data before relying on it. Do not promote PROPOSED items to DECIDED on your own.

## Implementation tracking

This section is the live implementation checklist. The design below remains the
source of truth; completed requirements are recorded here rather than removed,
so the rationale and privacy constraints remain auditable.

| Area | Status | Notes |
| --- | --- | --- |
| Basic transcript parsing | partial | Claude, Codex, Gemini, OpenCode and Aider parsers exist. Claude/Codex are the v1 targets; Gemini/Aider timing stays low-confidence. |
| Basic union time accounting | complete | Presence intervals are capped and concurrent activity is unioned. |
| Local canonical event store | partial | An append-only SQLite store and source discovery helpers exist; automatic multi-source daemon collection still needs wiring. |
| Deterministic dialogue trace and PESDO factors | complete | `formula.toml` is loaded by the local processor; explicit P/E/S/D permille arithmetic, O gate/soft configuration, fullcaps, dialogue types and bounded transition deltas are versioned and tested. |
| Filesystem/git provenance | partial | Pull-based derived commit/worktree summaries and a polling observer exist. File attribution, blame/survival and review adequacy remain. |
| Coach and evidence bundle | partial | Privacy-safe user/admin evidence views, local preview/upload, baseline comparison and rule-based tips exist. Tip-effect follow-up and episode deep links remain. |
| Server evidence verification | complete | Redacted append-only evidence bundles, provenance and authenticated user/admin reads reject raw prompt/code/path/tool-output fields. |
| GitHub gate, cross-user fraud checks, audits | partial | Public-repository/HEAD verification gates submission credit; always-on cadence, overlap, clock-skew and cross-user commit-dedupe findings feed manual review. Account linking, farm detection and replay/Merkle audits remain. |

1. Goals (priority order)
1.  DECIDED, primary: self-improvement and self-knowing. Measure how much time a user spends coding with agent harnesses, split it into episodes, show where they are weak, show how they improved versus earlier sessions, and suggest concrete signals and tips. The episode categories the owner named: productivity, contextless yelling, oneshotting, working side by side.
2.  DECIDED, secondary: stop fraud for Hack Club. HackAI must give the fraud department the same level of detail Hackatime gives, while staying usable for the majority of privacy-wanting users. The fraud team would prefer to see HOW the user talked to the agent.
3.  DECIDED: this is new standalone software. It does not need to run on or integrate with Hackatime. It is like Hackatime, but for agentic coding.
4.  DECIDED: this is a Hack Club AI time tracking bounty entry. Stack from earlier planning: Rust server plus JS web.
5.  DECIDED: the clock matters. The first version must be buildable in about a day, so build order below is part of the design.
2. Core principles
• DECIDED: the client is the user's machine and is adversarial. Nothing the client computes (hash, signature, rating) is proof. Server-observed evidence carries the real weight.
• PROPOSED: store events and features, never labels. Labels are derived, versioned, recomputable, so improving a rule relabels all history for free.
• PROPOSED: the processor is deterministic and versioned. Same input gives the same output. A reviewer can rerun it and diff.
• PROPOSED: coaching is local-first and uses rich local data. The fraud export is a redacted derived view of the same data. Know-heavy personal signals stay local and are excluded from the export by default.
• PROPOSED: describe the export as an evidence bundle, never as proof.
• DECIDED (my recommendation, owner agreed by continuing): the fraud-risk output is separate from the PESDO score and carries reason codes.
3. PESDO score
• DECIDED: score is the product P x E x S x D x O in permille. P measures human presence, E measures human-authored evidence, S measures information gain, D measures repetition and review-diligence penalties, and O is the submission outcome gate.
• DECIDED build order across the formula:
    ◦ P and E first: human presence plus evidence class gives a defensible number on day one.
    ◦ S next: simple counter logic, cheap once information events are parsed.
    ◦ D: needs SimHash and review-latency features. Ship with conservative penalties, or defer if the day runs short.
    ◦ O is a gate, not per-bucket scoring: a GitHub check at submission. In v1 O is 1000 or 0 at the submission step.
• DECIDED: use u128 and one divide at the end. Note: 1000^5 = 1e15 fits u64. u128 is justified only if you multiply by bucket duration before the divide (1e15 x 86,400 exceeds u64). The final divide is by 1000^4 and truncates (floor), documented in `formula.toml`.
• DECIDED: write formula.toml with O in gate mode and a documented switch for a soft factor. The owner's message describing this was cut off after 'so turning'. OPEN: ask for the rest of the O switch description.
• DECIDED: the dialogue-trace deltas (section 11) feed S and D, not a separate additive scheme, so the two do not fight the multiplicative formula.
4. What was dropped or demoted from the earlier stack
• DROPPED, skill-file sha256 used as a pepper (changed skill file marks request forged). Reason: every user has the same file, so its hash is a public version identifier, not a secret, and anything the client computes can be reproduced by the person controlling the client.
• DEMOTED, MCP server that nudges the agent to rate the user every 5 hook events. Reason: forgeable (skills, config and prompt are user-controlled). Kept only as optional low-weight reference data (section 12), never a score input or gate.
• DEMOTED, local ML model plus regex classifier for prompts. Reasons: local means client-forgeable, tone is not the target, padding defeats it, reproducibility is poor, no labeled eval set. Order of work: grounded features and rules first, hand-labeled eval set second, ML only as a tiebreaker for ambiguous episodes with the model hash pinned per score. OPEN: whether the old model trained on labeled data.
5. Architecture (PROPOSED, owner accepted the direction)
1.  Collectors: transcript tailers per harness, git (pull-based), and a filesystem watcher, all in a local daemon. Normalized events go to one append-only local SQLite store.
2.  Processor (deterministic, versioned): episode cutting, grounded features, turn labeling, scoring from formula.toml.
3.  Coach: local UI or terminal summary. This is the product.
4.  Export (opt-in): redacted evidence bundle.
5.  Server (Rust, thin): ingest and verification only. GitHub cross-checks, dedupe, cross-user collision checks.
6.  Web (JS): dashboards for the user and for reviewers.
Adapter design: one adapter trait per harness emitting a normalized event (timestamp, kind, text hash, token count, etc). The scoring code only sees normalized events. Preserve unknown record types as raw values and never hard-fail on them.
6. Data sources (DECIDED: core signals must be always-on, no per-harness setup)
Always-on sources: on-disk artifacts, git, and the filesystem watcher. Hooks, OTel and MCP are optional enrichers (section 7), never required.
Claude Code (verified from docs and third-party parsers)
• Append-only JSONL at ~/.claude/projects/<encoded-project-path>/<sessionId>.jsonl. Entries wrap an API message in an envelope with uuid, parentUuid, timestamp, sessionId, isSidechain, cwd, gitBranch.
• Entry types include user, assistant, system, summary, file-history-snapshot, queue-operation. queue-operation entries (prompts typed while the agent is busy) are a presence signal.
• Content blocks: text, thinking, tool_use. Assistant entries carry model and token usage.
• Subagents live under <project>/<parent-uuid>/subagents/<agent-id>.jsonl. Compaction artifacts (agent-acompact-*) must be skipped to avoid double counting.
• ~/.claude/history.jsonl is a separate prompt log (display text, timestamp, project).
Codex CLI
• Rollout JSONL at ~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl (or under $CODEX_HOME/sessions).
• Line types: session_meta, turn_context, response_item, event_msg, session_state, compacted. event_msg covers user messages, agent messages, token counts.
• history.jsonl indexes user messages (session_id, ts, text). The session header records who launched it (TUI, exec, IDE plugin).
• UNVERIFIED: one developer found v0.130 traces lacked exec begin/end events and had no causal parent field. Parse defensively.
OpenCode
• Current versions: SQLite at ~/.local/share/opencode/opencode.db (or $XDG_DATA_HOME/opencode/, or OPENCODE_DB). Tables session, message, part. Sessions have directory, title, version, time_created, time_archived, model. Messages have time_created plus a JSON blob (role, modelID, finish, tokens). Parts are typed JSON.
• Older layout: per-record JSON files under a storage/ directory (migrated). Support both, SQLite first.
• Open the DB read-only (it is written while the app runs).
Gemini CLI (phase 2)
• Newer: ~/.gemini/tmp/<project>/chats/session-*.jsonl. Legacy single-JSON files converted on resume. Project dir is a path hash or slug. There is no end-of-turn marker.
Aider (phase 2, low confidence)
• Per-project markdown: .aider.chat.history.md and .aider.input.history in the repo. User prompts appear as > blocks. Session start header has a timestamp. Parsers found show no per-message timestamps, so turn latency and idle gaps are not recoverable. UNVERIFIED: confirm on a real file.
• --llm-history-file logs full LLM conversation but is off by default: optional enrichment only.
• UNVERIFIED: analytics_log and attribute_author options appeared in a config dump. Check what they emit and whether commits are marked.
• Give Aider a lower evidence class or low-confidence P. Do not fake precision. Do not emit 'side by side' for Aider-only data.
Git (pull-based, no hooks)
• Commits, diff stats, reflog (reverts and resets), blame (code survival and authorship), commit cadence.
Filesystem watcher (in the daemon)
• Detects human edits: file changes with no matching tool edit nearby. This unlocks 'working side by side' and human-authored line share.
Practical rules
• Ingest continuously. UNVERIFIED: Claude Code may delete old transcripts after a retention period, so do not rely on batch scanning later.
• Tolerate a partial last line in JSONL and markdown files.
• Phase order: Claude Code first, then Codex and OpenCode, then Gemini and Aider.
7. Optional enrichers (not required, verified from docs)
Claude Code OpenTelemetry (opt-in; config read at startup only)
• Metrics: session count, cost, token usage (input, output, cacheRead, cacheCreation), lines of code added/removed, commit count, pull request count, code edit tool decisions (accept/reject, with tool and language), active time in seconds.
• Active time excludes idle but increments both while typing prompts and while receiving responses, so it is not pure human presence.
• Events: user_prompt (prompt_length; text redacted unless OTEL_LOG_USER_PROMPTS=1), tool_result (tool_name, success, duration_ms, error), tool_decision (decision accept/reject, source one of config, user_permanent, user_temporary, user_abort), api_request, api_error, assistant_response. Optional traces (beta) link each prompt to API calls and tool runs.
• user_abort gives interrupts directly. user_permanent versus user_temporary gives approval fatigue.
Claude Code hooks
• Documented events include UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, PermissionRequest, Stop, and others. Payloads carry session_id, transcript_path (missing on some events, and the transcript may lag), permission_mode, effort, and a prompt_id (newer versions). UNVERIFIED: exact permission_mode values and Stop exit-code semantics.
Codex OpenTelemetry (opt-in [otel] block in ~/.codex/config.toml)
• Events: api_request, sse_event (durations, status, token counts), user_prompt (length; text redacted unless log_user_prompt = true), tool_decision (approved, approved_for_session, denied, abort; source config or user), tool_result (duration_ms, success, output).
• A third-party adapter says Codex notify hooks only fire on turn completion and approval requests. UNVERIFIED and possibly outdated. Check current Codex.
• Config values readable as context features: approval_policy, sandbox_mode, model_reasoning_effort.
Design option (PROPOSED)
• A local OTLP receiver in the daemon could ingest both tools through one path when users opt in. Cross-check OTel counts and timings against transcript data.
8. Normalized event record and upload privacy
Target (DECIDED goal): event-level granularity like Hackatime heartbeats, so reviewers can drill down and recompute durations. Privacy comes from redacting fields, not from dropping detail.
Event record (PROPOSED fields):
• timestamp (keep second-level precision; the gap math and 2-minute cap need it), hashed session id, harness and version, event kind (prompt submit, agent turn end, tool call kind, human edit, test run, commit), project name, file reference, language or extension, lines added and removed, branch, hashed machine id, test exit code.
• Prompt events: length, anchor count, fullcaps flag, SimHash. No text.
Privacy defaults (DECIDED requirement: usable for most privacy-wanting users, so private mode is the default):
• Default upload is derived features only. No prompt text, no code, no paths, no cwd (project name only).
• Grounding checks (does this path exist, did this error appear in a prior tool result) run locally. Only the result goes up.
• PROPOSED: file references as a per-user salted hash by default (reviewers still see 'edited 40 times across 3 sessions'); real names opt-in. OPEN: does the fraud team accept hashed references, or need names to spot copied repos?
• PROPOSED: a tool preview command prints the exact JSON that would upload. Publish the schema.
• PROPOSED: 64-bit SimHash per prompt only above a length floor (very short prompts are guessable). Enough to detect many accounts running one script.
• PROPOSED: fixed retention window, then delete or aggregate. Export and delete on request. Opt-in redacted excerpt sharing with secret scanning before anything leaves the machine.
• DECIDED direction: instead of storing prompts, store the type of prompt and classify the agent's replies too (section 11).
9. Episodes and time accounting (PROPOSED)
• Cut episodes on idle gap (start at 15 minutes, configurable), session boundary, or a commit.
• Per turn, split time into agent time (prompt submit to last assistant event), human time (gap before next prompt, capped), and idle.
• Hours derivation follows the Hackatime model: duration is derived, never stored. Rule: presence events only credit time, gap between consecutive presence events is capped (Hackatime uses 120 seconds), first event contributes zero. Agent-only tool events do not credit time by themselves.
• Concurrent sessions are credited as the union of intervals, never the sum (human attention is single-threaded).
• Cap idle credit per bucket. Do not punish a long gap before a high-information prompt.
• Compute human gaps from the real end of agent work, not from the end of any nudge turn.
10. Episode labels (PROPOSED rules, owner-named categories)
Labels are derived per turn, then time-weighted majority per episode, stored with confidence, reason codes and rule version.
• Rule order: contextless yelling, then side by side, then oneshotting, then productive.
• Contextless yelling: high SimHash similarity to the previous prompt, short anchor-free prompts, quick resubmits after a failed tool result, fullcaps (only as a confidence raiser).
• Oneshotting: owner's term is low-specification delegation; split into accepted (commit and green follow) and correction spiral. Experts oneshot too, so do not diagnose the user.
• Side by side: human-authored changes (git plus watcher) interleaved with prompts on the same files. Impossible without the watcher.
• Productive: default, but needs positive evidence (new information per prompt, tests passing, commits, low repetition).
• OPEN: is the label per episode, or minutes split across labels within an episode?
• OPEN: should the oneshot flag look only at the first prompt of a session, or any prompt that fires a long agent run?
Prompt signal rules
• DECIDED: the only caps measure is fullcaps (not tone, not caps ratio). Tech terms are skipped by default.
• Fullcaps rule (PROPOSED details): strip code fences, inline code, URLs, file paths, quoted tool output. Skip tech tokens: a static allowlist (HTTP, API, JSON, SQL, ENOENT, SIGSEGV, README, TODO), any token with an underscore or digit, and a dynamic allowlist of caps tokens appearing verbatim in the repo, prior tool results or earlier assistant text. Count cased letters only (caseless scripts never fire). Fire if at least 8 cased letters remain and none are lowercase. Bare 3 to 5 letter acronyms in no list (WHY, NOW) count as non-tech. Start with whole-prompt only. OPEN: whole prompt versus a fully capitalized sentence inside a longer prompt.
• Fullcaps is trivially evaded, so it only raises 'contextless yelling' confidence when it co-occurs with anchor-free text or a repeat. It never labels alone.
• Contextless 'fix it' or 'do that' is judged relative to the previous assistant turn: right after a failing test or error it is a legitimate continuation. Short answers to agent questions are fine (check whether the last assistant message ended with a question or choice).
• Grounded anchors: file paths and identifiers that exist in the repo, line numbers, quoted error strings that appeared in a prior tool result. Grounding defeats padding with fake context.
• Repetition: SimHash against the previous prompt, judged together with information gain (a good retry changes something).
11. Dialogue trace (DECIDED direction, PROPOSED detail; owner said the sample dialogue was boilerplate to be refined)
Purpose: show how the user talked to the agent with no prompt text. Owner's boilerplate sample: U asks to build; A builds; U yells without context; A gets another thing wrong; U explains what was wrong; then the PESDO score takes a delta from plus or minus values. OPEN: does 'delta final' mean per-turn signed deltas summed, or end state minus start state?
User acts (closed enum, versioned): specify, nudge (continue or retry), report_failure, steer, decide (answers an agent question), approve, reject_or_revert, interrupt, ask_to_understand (a positive learning signal), meta (about the tool itself). No topic or free-text tags.
Failure-report context vector: three booleans, evidence (error text, test output or diff reference that exists in the transcript), expectation (states what should happen), location (names a file, function or line that exists). Contextless means all three false.
Agent states (objective signals first, claims second): asked, planned, explained, edited (verified green, verified red, unverified), claimed_done_unverified, tool_error, blocked, looping (same edit or failure repeated).
Transition deltas live in formula.toml as a signed table keyed on (agent state, user act). Illustrative values only, not decided: claimed done but red then contextless report is -2; claimed done but red then contextful report is +2; claimed done green then approve is +1; claimed done red then approve (rubber stamp) is -2. Rules: score only the user's response to the agent state, never penalize the user for agent errors. Keep process (summed user-attributable deltas, bounded per episode) separate from outcome (shipped, abandoned, ended red) and recovery time (turns from first red to green).
Per-turn record (no text): user_act, act_conf, ctx{evidence, expectation, location}, new_info_bucket, len_bucket, fullcaps, repeat_dist_bucket, answers_question, latency_bucket, agent_state, verification, files_changed_bucket, tool_errors, reply_to, episode_id, rule_version.
Calibration: hand-label about 100 episodes, then relabel a few days later. Self-disagreement shows where the enum is ambiguous.
12. Agent opinion (optional reference data)
• DECIDED: fire the agent's opinion of the user and keep it as reference data. Mechanism chosen: a Stop hook after the agent finishes, limited to 100 tokens. Earlier the owner said every 5th hook. OPEN: cadence (every 5th Stop, every 5th user prompt, or episode end).
• Because hooks are opt-in, this is an optional layer. The pipeline must not depend on it.
• PROPOSED implementation: check the stop_hook_active flag to avoid loops. Embed a nonce in the nudge so the processor drops that turn from labels and features. Have the agent call a record_opinion MCP tool with structured args (label enum, confidence bucket, 2 to 3 reason codes, cited turn indices, not uuids). Truncate oversized output. Record model, harness version and nudge prompt version. Store in its own table keyed to the episode. Never feed it back into labels, scores, tips, or later agent context.
• Uses: calibration (agreement with deterministic labels per label), picking disagreements for hand-labeling, drift detection after model or harness updates. Known bias: LLM raters are generous and favor polite or long prompts, so it never gates or scores.
13. Slop score
• DECIDED definition: slop is something oneshotted AND entirely built by the agent with minimal or no human interaction. It also incorporates, per the owner: too-early-finished review, and huge diffs.
• PROPOSED formula: slop = oneshot x agent_authorship x (1 - engagement), in permille. Output also a confidence and top reason codes.
• Oneshot: share of surviving lines (via blame) introduced by the first agent run of the episode, plus prompts per commit. A 30-prompt session ending with 95 percent of lines from run one is still a oneshot.
• Agent authorship: match Edit/Write payloads from the transcript to the final diff with whitespace normalization. Unmatched lines are human or unknown (the watcher helps).
• Engagement: specify, steer and decide acts, human edits to the diff, ask-to-understand turns, and review adequacy.
• Review adequacy = min(1, actual / expected). Expected review time scales sublinearly with effective changed lines (exclude lockfiles, generated files, renames, formatter noise), with a per-line rate and a floor in formula.toml. Actual review time is the gap between the agent's last edit and the next human action, capped. In auto-accept mode measure at commit time.
• Huge diffs: diff mass produced by one agent run between human actions, log-scaled with saturation. PROPOSED: interaction-only (huge raises the required review time, adequacy does the work). OPEN: should huge diffs also raise the score on their own?
• Reason codes: EARLY_REVIEW, HUGE_DIFF, plus component codes.
• Edge cases: squash and rebase (match on content, not hashes), lockfiles, generated files, CLI scaffolder output, harnesses you do not parse (mark low confidence, not zero slop), Aider auto-commits (attribute by content).
• Naming note: this measures autopilot, not quality. Show a separate verification modifier (tests run, commit while red, code survival) so the coach can say 'high autopilot, unverified' versus 'high autopilot, verified'. Let users tag a repo as exploratory.
• OPEN: should slop feed the D factor, or remain a separate output? OPEN: does a long detailed one-prompt spec plus one run still count as a oneshot?
• Calibration: hand-label about 50 commits as slop or not and check ranking before trusting weights. Weights are v0 until then.
14. Signal catalog
Tags: F = fraud-review value, P = parse effort from always-on data, I = Improve (a lever the user can change), K = Know (durable personal pattern). Levels H, M, L.
Time and presence
• Turn timing, human gaps, run length: F H, Easy, I M, K H
• Concurrent sessions (union): F H, Easy, I M, K M
• Cadence regularity: F H, Easy, I L, K L
• Time of day, hours into session: F L, Easy, I H, K H
Prompt and dialogue
• User act enum and context vector: F M, Med, I H, K H
• Grounded anchors: F M, Med, I H, K M
• Repetition within user: F M, Easy, I H, K M
• Repetition across users: F H, Easy, I L, K L
• Fullcaps: F L, Easy, I L, K M
• Agent state enum: F L, Med, I M, K L
Verification and loops
• Test and build outcomes, recovery time: F M, Med (exit codes may be unstructured), I H, K M
• Looping: F L, Med, I H, K L
• Tokens per failed loop: F L, Easy, I M, K L
• Session decay and compaction timing: F L, Easy, I H, K M
Git and outcomes
• Commits, diff stats: F H, Easy, I M, K M
• GitHub push times and dedupe: F H, Easy, I L, K L
• Reverts and rework: F M, Med, I H, K M
• Code survival: F M, Hard (blame pass), I H, K H
• Commit while red: F M, Med, I H, K L
Authorship
• Agent versus human lines: F H, Hard, I M, K H
• Human edits via watcher: F H, Med, I M, K H
Oversight and slop
• Review adequacy: F H, Med, I H, K H
• Huge diffs and big runs: F M, Easy, I H, K M
• Oneshot share: F H, Med, I H, K H
• Slop composite: F H, Hard, I M, K H
• Interrupts and plan-first rate: F L, Med, I H, K M (interrupt markers UNVERIFIED)
Reference
• Agent opinion: F L, Easy, I L, K M
• Batch hash chain, processor hash: F M, Easy, no coaching value
Additional candidate signals discussed: permission mode and approval rates, wasted agent work (reverted or discarded diffs), tokens and cost per shipped commit, verify-before-commit, sensitive-file edits accepted with very low review latency, compaction and clear usage, subagent and skill and MCP usage, context switching and project switches, recurring failure fingerprints (same error signature across sessions, points to a knowledge gap), learning trend (anchor density and ask-to-understand over weeks), effort and model choice versus retries.
15. Coach design (the product)
• Show minutes per label, and trends against the user's own rolling baseline of the last N sessions, not just the previous session.
• Rank patterns by minutes lost times controllability. Show only the top two or three. Each links to a real example episode and carries one concrete tip.
• Close the loop: after a tip, track whether the metric moved over the next N sessions and say so honestly, including 'no change yet'.
• Confounders: task difficulty drives much of this. Let users tag a task or repo as exploratory or routine. Present patterns, not diagnoses.
• Example tips: vague starts lead to spirals, so state acceptance criteria up front; retry chains with no new info, so paste the failure output or revert and re-plan; long-session decay, so restart or compact at your personal threshold; rubber-stamping large diffs, so use smaller steps with checkpoints; waiting time, so queue the next task during long runs; late-night sloppiness.
• Day-one coach set (Easy or Med parse, high I or K): timing and time-of-day, user acts with context vector, anchors, within-user repetition, session decay, test recovery, review adequacy, oneshot share.
• OPEN: should the dashboard lead with Improve (what to change) or Know (what you are like)?
• DECIDED for v1: suggestions are rules plus templated text. An LLM remains optional later and may only lower/veto, never raise, a score.
• Any LLM that writes tips or judges evidence gets a fixed output schema, runs without user skills or config in context, validates that cited ids exist, treats packet text as data (ignore instructions inside it), and may only lower scores or veto, never raise them.
16. Fraud evidence bundle and trust model
Goal (realistic): stop cheap, scalable fraud (automation, double-dipping, copied repos) and make expensive fraud costly. It is a risk ranker for human reviewers, not a lie detector.
Bundle contents (PROPOSED): claimed hours with derivation (active human, agent, idle per episode; union of concurrent sessions); timeline mapping episodes to commits and pushes; reason-coded flags with measured value and the threshold crossed; provenance (processor version and hash, adapter and harness versions, hash chain over upload batches); the dialogue trace enums; server-side verification results.
Fraud patterns and detectors:
• Unattended loops or scripted prompts: no human edits, near-constant cadence, templated prompts, zero review-latency variance, session launch mode (Codex records the launcher; UNVERIFIED whether Claude Code headless runs are distinguishable).
• Concurrency inflation: union of intervals.
• Hand-fabricated transcripts: parentUuid chain integrity, monotonic timestamps, valid model and version strings, token usage plausible for elapsed time. A careful forger passes these, so they only filter lazy ones.
• Copied or bulk-generated repos: giant single initial commit, template forks, lines per claimed hour above a plausibility ceiling.
• Double-dipping: dedupe commit hashes across the whole program.
• Farms and templates: cross-user SimHash collisions, shared IPs and datacenter ranges (needs population-level data).
Server-observed evidence (the strong part): GitHub push times and webhook receive times (commit author timestamps are forgeable, do not trust them); claimed edits must appear in the actual diff and the diff must be explained by claimed activity; public repo and linked commits mandatory for credited hours.
Audit mechanics (later): per-batch hash chain (stops retroactive rewrites and exposes gaps, but does not stop forging forward); Merkle commit-and-challenge (commit a Merkle root over per-event feature records at upload, server picks random indices afterward, user reveals only those records with proofs; no content leaks because records are already redacted); random replay audits. OPEN: will the fraud team accept challenge-response audits instead of raw prompt access?
Policy: assume rules are public (forgers read the code), so thresholds, sampling seeds and cohort baselines are server-side config. Tiered outcomes only (pass, sampled review, hold), never auto-ban, because many submitters are teenagers on unusual setups. Every flag shows reason codes and evidence with an appeal path. Cap credit per bucket. OPEN: does Hack Club have a human review queue downstream? Does the server afford a model call per episode, or must judging be batched per session?
17. Hackatime reference (parity target)
• Hackatime receives heartbeats from WakaTime-compatible editor plugins. Duration is derived from ordered heartbeat timestamps, first heartbeat contributes zero, each later one contributes its gap capped at 120 seconds. Admin exclusion rules can hide heartbeats without deleting them. Users carry a trust factor, and trust conviction changes are restricted to high admin levels. The legacy README lists file names among the data sent.
• Parity means reviewers get: per-user timeline, per-project and per-day rollups, event-level drill-down, concurrent-session overlap view, flags with reason codes and thresholds, and a published deterministic duration rule anyone can recompute from events.
• Mapping flags onto trust-factor input and heartbeat exclusion is a PROPOSED nicety, not a requirement.
• UNVERIFIED: the exact WakaTime heartbeat field list and whether it has an 'AI coding' category.
18. Build order for the day (PROPOSED, owner accepted direction)
1.  Claude Code adapter plus the local event store.
2.  Episode cutting and turn labeling.
3.  Hand-label a sample (about 100 episodes) and check the rules against it.
4.  Coach view with tips.
5.  Export bundle, then the GitHub gate (O).
6.  Second adapter (Codex or OpenCode) if time remains.
Fraud bundle day-one set (Easy parse, high fraud value): timing, union of concurrent sessions, cadence regularity, commits, GitHub push times, commit dedupe, cross-user SimHash. Authorship and slop come later since they depend on the hardest parsing. Merkle audits and hash chains are not v1.
19. Verify before relying (UNVERIFIED list)
• Do interrupts and permission denials appear as recognizable markers in Claude Code transcripts? (I believe a synthetic 'request interrupted' message exists.)
• Are Bash exit codes structured in tool results, or only in output text? This decides green/red detection.
• Do Codex rollouts include explicit tool begin/end events on current versions?
• Transcript retention and deletion behavior for each harness.
• Can Claude Code headless runs be distinguished from interactive ones?
• Current Codex hook support (the notify-only claim may be outdated).
• Hook permission_mode values and Stop-hook exit-code semantics.
• Aider per-message timestamps, analytics_log, attribute_author.
• Whether Claude Code's statusLine JSON exposes useful per-session fields.
• Hackatime heartbeat field list and WakaTime AI-coding category.
• Quick check: run jq over your own Claude Code session files, looking at interrupted turns and failed-test turns.
20. Open questions for the owner (consolidated)
1.  What do P, E, S, D, O stand for, and what was the rest of the O soft-factor switch description?
2.  Floor or round on the final divide?
3.  Label per episode, or minutes split across labels within an episode?
4.  Oneshot flag: first prompt of a session only, or any prompt that fires a long agent run?
5.  Fullcaps: whole prompt only, or also a fully capitalized sentence in a longer prompt?
6.  What does 'delta final' mean: summed per-turn deltas or end minus start?
7.  Agent opinion cadence: every 5th Stop, every 5th user prompt, or episode end?
8.  Slop: feeds D or separate output? Huge diffs interaction-only or also additive? Does a detailed spec plus one run count as oneshot?
9.  Dashboard emphasis: Improve first or Know first?
10.  Tips generated by rules and templates, or by an LLM?
11.  Fraud team: accept hashed file references, challenge-response audits, derived features only? Is there a human review queue? Can the server afford a model call per episode?
12.  Did the old local ML model train on labeled data?
Appendix A: Datapoint catalog (datapoints only)
Collect everything below that the harness or git exposes. Tags: [X] derived and safe to export by default, [L] local only, [S] computed server-side. Anything not marked [L] contains no prompt text, code or raw paths. Missing sources leave the field null with a confidence flag, never zero.
A1. Event identity and context
• event_id [X], ts_utc_ms [X], ts_seq_idx [X], source_kind (transcript, git, fs_watch, otel, hook) [X]
• harness (claude_code, codex, opencode, gemini, aider) [X], harness_version [X], adapter_version [X], schema_version [X]
• session_id_hash [X], parent_session_id_hash [X], is_sidechain [X], subagent_id_hash [X], parent_event_uuid [L], uuid_chain_valid [X]
• event_kind enum: prompt_submit, queue_op, agent_turn_end, tool_call, tool_result, permission_request, permission_decision, interrupt, human_edit, test_run, build_run, commit, compaction, clear, resume, session_start, session_end [X]
• model [X], model_family [X], model_switch_flag [X], reasoning_effort [X], permission_mode [X], approval_policy [X], sandbox_mode [X]
• launch_mode (interactive, exec, ide, headless) [X], entrypoint [X], os [X], tz_offset_min [X], machine_id_hash [X]
• project_name [X], repo_id_hash [X], branch [X], cwd_hash [L], source_file_hash [L], source_byte_offset [L], raw_unknown_type_flag [X]
A2. Time and presence
• human_gap_ms, human_gap_capped_ms, idle_ms, idle_cap_applied, credited_presence_ms, credited_agent_only_ms [X]
• agent_run_ms, time_to_first_agent_event_ms, time_to_first_tool_ms, tool_duration_ms (per tool), api_latency_ms [X]
• queue_op_count, prompts_while_agent_busy, interrupt_latency_into_run_ms [X]
• concurrent_sessions_count, overlap_ms, union_interval_ms, sum_interval_ms, concurrency_ratio [X]
• hour_of_day, day_of_week, hours_into_session, session_start_ts, session_end_ts, session_duration_ms, gap_since_last_session_ms [X]
• late_night_share (23:00 to 05:00), weekend_share, active_day_streak, working_hours_histogram [X]
• cadence stats over inter-prompt gaps: gap_mean, gap_median, gap_std, gap_cv, gap_min, gap_entropy, gap_autocorr, share_gaps_under_1s, periodicity_score [X]
A3. Prompt features (no text)
• size: len_chars, len_tokens_est, line_count, word_count, len_bucket [X]
• pasted content: code_fence_count, pasted_block_len, pasted_ratio, url_count, submitted_via (typed, pasted, history_recall) [X]
• anchors: path_count, grounded_path_count, ungrounded_path_count, identifier_count, grounded_identifier_count, line_ref_count, error_quote_count, grounded_error_quote_count, anchor_count_total, anchor_density [X]
• fullcaps: fullcaps_flag, fullcaps_letters_count, caps_tech_skipped_count, caps_dynamic_allowlist_hits [X]
• shape: question_mark_count, exclamation_count, imperative_start_flag, pronoun_only_flag, short_template_flag (fix it, do that style), script_caseless_flag [X]
• dialogue links: answers_agent_question, prev_assistant_ended_with_question, prev_agent_state [X]
• repetition: simhash64 (above length floor) [X], simhash_dist_prev, simhash_dist_min_session, simhash_dist_min_user_30d, near_dup_count_window, repeat_chain_len [X]
• information: new_info_bucket, info_gain_tokens_new, specificity_score, has_constraints, has_acceptance_criteria, has_tech_choice, mentions_test, mentions_file, domain_term_density, scope_size_bucket [X]
• commands: slash_command_name (builtin only) [X], custom_command_count, skill_invocation_count, attachment_count, image_count, rewind_or_edit_prev_flag, interrupt_flag [X]
• prompt_text [L] (local coach only, never uploaded by default)
A4. Agent turn
• text and tokens: assistant_text_len, thinking_block_count, thinking_tokens, tokens_in, tokens_out, tokens_cache_read, tokens_cache_creation, cost_est, context_tokens_at_turn, context_fill_ratio [X]
• tool calls by kind: read, edit, write, bash, grep, glob, web, subagent_task, mcp (server hashed), todo, notebook: tool_call_count_by_kind [X]
• files: distinct_files_read, distinct_files_edited, files_created, files_deleted, files_renamed, lines_added, lines_removed, net_lines, edit_count, repeated_edit_same_region_count [X]
• bash: bash_command_count, bash_exit_codes, test_run_flag, build_run_flag, lint_run_flag, typecheck_run_flag, install_run_flag, git_command_flag, network_command_flag, destructive_command_flag [X]
• errors: tool_error_count, tool_error_kinds, tool_retry_count (same tool and args), api_error_count, api_retry_count, refusal_flag, stop_reason [X]
• permissions: permission_request_count, permission_denied_count, approve_latency_ms, always_allow_count, auto_accept_flag, user_abort_flag [X]
• state: agent_state enum (asked, planned, explained, edited_green, edited_red, edited_unverified, claimed_done_unverified, tool_error, blocked, looping), claim_done_flag, claim_verified_flag, plan_emitted_flag, asked_question_flag, todo_update_count, looping_flag [X]
• scale: subagent_spawn_count, mcp_tool_call_count, compaction_event_flag, clear_event_flag [X]
A5. Verification
• test_framework_detected, tests_run, tests_passed, tests_failed, tests_skipped, test_exit_code, test_duration_ms, build_duration_ms, lint_warning_count, type_error_count [X]
• first_red_ts, first_green_after_red_ts, recovery_turns, recovery_ms, fail_to_pass_count, pass_to_fail_count, flaky_flag, error_signature_hash (recurrence across sessions) [X]
• commit_while_red, commit_no_test_since_edit, tests_added_vs_code_added, test_files_edited_share, coverage_pct (if available) [X]
A6. Git and outcomes
• commit: commit_hash_hash, author_ts, committer_ts, push_ts [S], parent_count, is_merge, files_changed, insertions, deletions, effective_lines (lockfiles and generated excluded), renames, binary_files, file_type_distribution, dirs_touched, signed_flag [X]
• message: msg_len, msg_conventional_flag, agent_coauthor_trailer_flag [X], msg_text [L]
• rhythm: time_since_prev_commit_ms, commits_per_active_hour, commits_per_prompt, prompts_since_last_commit, agent_runs_since_last_commit, human_edits_since_last_commit, first_commit_of_repo_flag, initial_commit_size, bulk_add_flag [X]
• history rewriting: amend_count, force_push_flag, rebase_flag, squash_flag, revert_commit_flag, reset_event_count (reflog), checkout_discard_count, stash_count, branch_create_count, branch_delete_count, tag_count [X]
• remote: pr_opened_flag, ci_status [S], github_push_event_ts [S], commit_dedupe_hit_count [S], repo_public_flag [S], template_fork_flag [S]
• survival: ai_lines_surviving_1d, ai_lines_surviving_7d, ai_lines_surviving_30d, survival_rate, agent_line_half_life_days, lines_rewritten_within_15m, lines_rewritten_within_1h, lines_rewritten_within_1d, file_churn, files_edited_again_count, human_rewrite_of_agent_lines_share, deleted_agent_lines_share [X]
• authorship: agent_authored_lines, human_authored_lines, unknown_lines, matched_by_content_share, formatter_normalized_flag, mixed_hunk_count, agent_share_by_commit, agent_share_by_file, agent_share_by_language [X]
A7. Review and oversight
• review_latency_ms (agent last edit to next human action), expected_review_ms, review_adequacy_permille, diff_mass_per_run, effective_lines_per_run, files_per_run, run_without_checkpoint_flag [X]
• human_edit_to_agent_diff_within_5m, ask_to_understand_after_big_diff, approve_without_review_count, reject_count, undo_or_rewind_count [X]
• sensitive_file_edit_flag (auth, env, config, CI, deps, migrations), sensitive_edit_review_ms, sensitive_accepted_fast_flag [X]
• interrupt_count_per_run, interrupt_early_redirect_flag [X]
A8. Dialogue trace (per turn)
• user_act enum: specify, nudge, report_failure, steer, decide, approve, reject_or_revert, interrupt, ask_to_understand, meta [X]
• act_conf, ctx_evidence, ctx_expectation, ctx_location, ctx_score (0 to 3) [X]
• agent_state, verification (green, red, none), files_changed_bucket, tool_errors, reply_to [X]
• transition_key, transition_delta, process_score_running, rule_version [X]
A9. Episode
• episode_id, start_ts, end_ts, duration_ms, active_human_ms, agent_ms, idle_ms, turn_count, prompt_count [X]
• label, label_conf, label_reason_codes, label_rule_version, label_minutes_split (productive, yelling, oneshot, side_by_side) [X]
• first_prompt_specificity, retry_chain_max, correction_spiral_flag, spiral_depth, oneshot_share, prompts_to_first_commit [X]
• outcome (shipped, abandoned, ended_red), ended_green_flag, recovery_ms [X]
• human_vs_agent_edit_share, side_by_side_ms, interleave_count (human edit between prompts), rework_rate [X]
• yelling_turn_count, contextless_turn_count, approve_without_review_count, tests_run, tool_failure_count, abort_count [X]
• files_in_episode, lines_in_episode, tokens, cost_est [X]
• user_tag (exploratory, routine, other) [X], task_type_tag [L]
A10. Session
• duration_ms, turn_count, episode_count, label_minutes_split, commit_count, prompts_per_commit [X]
• compaction_count, first_compaction_turn, clear_count, resume_flag, continued_from_previous_flag [X]
• tokens_total, cost_est, peak_context_fill, correction_rate_by_age_decile [X]
• models_used, subagent_use, mcp_use, skill_use [X]
• agent_opinion_samples (reference only) [X]
A11. Rollups and trends (per day, week, project, user)
• per day: credited_hours, agent_hours, idle_hours, sessions, commits, lines, label_minutes, slop_mean, review_adequacy_mean, recovery_median, yelling_rate, oneshot_rate, tokens, cost_est [X]
• per week: delta of each metric against the rolling baseline of the last N sessions, plus best_hours, worst_hours, project_switches_per_hour, peak_concurrent_sessions [X]
• learning trend: anchor_density_trend, ask_to_understand_rate_trend, prompts_per_commit_trend, time_to_first_green_by_repo [X]
• failure fingerprints: error_signature_recurrence_count, error_signature_first_seen [X]
• tip loop: tip_id, tip_shown_ts, tip_metric_name, metric_before, metric_after_n_sessions, tip_effect (improved, no_change, worse) [L]
A12. Slop score
• oneshot, agent_authorship, engagement, review_adequacy, huge_diff_mass, verification_modifier, slop_permille, slop_conf, slop_reason_codes (EARLY_REVIEW, HUGE_DIFF, ONESHOT, AGENT_AUTHORED, LOW_ENGAGEMENT, UNVERIFIED) [X]
• repo_tag_exploratory_flag, excluded_by_glob_lines [X]
A13. PESDO
• p_permille, e_permille, s_permille, d_permille, o_permille, product_permille, evidence_class, o_gate_state, o_mode (gate, soft), rounding_mode, formula_version, intermediate_u128 [X]
A14. Fraud and risk (server side)
• concurrency_ratio_flag, cadence_cv_flag, templated_prompt_cluster_id, cross_user_simhash_collision_count, shared_ip_class, datacenter_ip_flag [S]
• claimed_hours_vs_push_activity, activity_to_commit_alignment, lines_per_claimed_hour, giant_initial_commit_flag, template_fork_flag, commit_dedupe_hits [S]
• parent_uuid_chain_valid, timestamps_monotonic, usage_plausibility_score, model_version_valid, headless_launch_flag, no_human_edits_flag [S]
• flag_codes_list with measured value and threshold crossed, risk_tier (pass, sampled_review, hold), reviewer_outcome, appeal_state [S]
A15. Provenance and integrity
• processor_hash, adapter_versions, client_version, batch_seq, prev_batch_hash, merkle_root, challenge_indices, merkle_proofs, clock_skew_estimate_ms, upload_ts, preview_approved_flag [X]
A16. Agent opinion (optional reference table)
• opinion_label, opinion_conf_bucket, opinion_reason_codes, cited_turn_indices, opinion_model, opinion_harness_version, nudge_prompt_version, nudge_nonce, trigger_cadence, opinion_tokens_used, agreement_with_rule_label, disagreement_flag, stop_hook_active_seen [X]
A17. OTel enrichers (optional, if user enabled)
• Claude Code: session_count, cost_usd, token_usage_by_type, lines_of_code_added_removed, commit_count, pull_request_count, code_edit_decision_by_tool_language, active_time_s; events user_prompt_length, tool_result_success_duration_error, tool_decision_source (config, user_permanent, user_temporary, user_abort), api_request, api_error, assistant_response [X]
• Codex: api_request, sse_event_tokens_duration, user_prompt_length, tool_decision (approved, approved_for_session, denied, abort; source config or user), tool_result_duration_success [X]
A18. Repo and environment context
• language_mix, repo_loc, repo_age_days, contributor_count, test_presence_flag, ci_presence_flag, monorepo_flag, framework_detected, domain_bucket (web, firmware, systems, data, other, by extension) [X]
• agent_instructions_file_present (CLAUDE.md, AGENTS.md), agent_instructions_size, agent_instructions_change_count, hooks_configured_count, mcp_servers_configured_count, skills_configured_count, default_permission_mode [X]
A19. Explicitly not collected
• prompt text and code in the export, raw file paths in the export, keystrokes, screenshots, clipboard, tone or sentiment scores, profanity detection (the owner decided caps-lock yelling only), biometric or device fingerprints beyond a hashed machine id.
Appendix B: Algorithms (v0 specs, PROPOSED)
All thresholds named in CAPS are config values in formula.toml or server config, not constants. Every number below is a starting guess to be calibrated on hand-labeled data. Every algorithm is deterministic, emits reason codes, and records the rule version. Inputs refer to datapoints in Appendix A.
B1. Credited time (union of presence intervals)
Output: credited_hours = human_active + agent_attended, both reported separately.
1.  Collect presence events for the user across all harnesses, sessions and projects: prompt submits, queue ops, human edits (watcher), human-triggered test runs, user permission decisions, commits. Sort by timestamp into one global timeline. Working globally is what prevents double counting concurrent sessions.
2.  For each consecutive pair of presence events, gap = t[i+1] - t[i]. If gap <= CAP_HUMAN (start at 120 s, the Hackatime rule), add interval [t[i], t[i+1]]. Otherwise add only the trailing interval [t[i], t[i] + CAP_HUMAN]. The first event of the timeline contributes nothing by itself.
3.  For each agent run (prompt submit to agent turn end), add attended time only if a presence event follows within IDLE_LIMIT (start at 15 min): add [run.start, min(run.end, run.start + AGENT_WAIT_CAP)] with AGENT_WAIT_CAP starting at 5 min. Agent-only events never create credit on their own.
4.  Merge all intervals with a sweep line into a union. human_active is the union of steps 2 intervals. agent_attended is the union measure of step 3 intervals minus its overlap with human_active.
5.  Also output sum_interval_ms (before union) and concurrency_ratio = sum / union for the fraud layer.
Edge cases: clock skew (clamp negative gaps to zero and flag), harnesses with coarse timing (Aider: mark the contribution low-confidence), sessions crossing midnight (split only at reporting time, never in the merge).
B2. Episode cutting and turn labeling
Output: episodes, a label per turn, a time-weighted label per episode, with confidence and reason codes.
Cutting: start a new episode when gap > IDLE_CUT (15 min), when the session id changes, or when a commit is followed by a prompt with simhash_dist_prev above the repeat threshold (a new topic).
Per-turn scoring, applied in this rule order, first match wins:
1.  CONTEXTLESS_YELL. Compute y = a*[anchor_count == 0 and not follows_error_or_question] + b*[simhash_dist_prev <= T_REP] + c*[latency < T_FAST and prev_agent_state in (edited_red, tool_error, claimed_done_unverified)] + d*[fullcaps_flag]. Label if y >= T_Y and at least one of the first two terms is true. Fullcaps alone never labels. Skip if answers_agent_question.
2.  SIDE_BY_SIDE. Label if human edit events land within W_SBS of the turn on files the agent touched (or the same repo) and the episode has at least K_SBS interleavings. Never emit for harnesses without the watcher.
3.  ONESHOT. Label if the prompt that triggered the run is low-specification (specificity_score < T_SPEC or scope bucket large) and engagement is low (no human edits, review_adequacy < T_ADEQ, at most 2 prompts before commit or episode end). Sub-label accepted (commit with green tests follows) or spiral (red or rework follows).
4.  PRODUCTIVE. Label only with positive evidence: info_gain >= T_INFO, a red to green transition, or a commit with tests run. Otherwise UNLABELED, not productive.
Episode label: weight each turn by its credited time (active plus attended), take the label with the largest weighted share. label_conf = winner_share * mean(turn_conf). Store the minutes split per label as well, since a single label hides mixed episodes.
B3. Slop score per commit
Output: slop_permille, a confidence, reason codes, and the components.
1.  E_lines = effective changed lines (exclude lockfiles, generated, vendored, formatter-only hunks, pure renames).
2.  Authorship: normalize whitespace, then match each added line (hash of line plus a small context window) against Edit and Write payloads in the transcript. agent_share = matched / E_lines. Unmatched lines are human or unknown, split further with watcher events.
3.  Oneshot: use blame to find the share of surviving lines in the diff that were introduced by the first agent run of the episode, first_run_share. If blame is unavailable, fall back to 1 / max(1, prompts_since_episode_start) with low confidence.
4.  Engagement as a noisy-OR of independent human-involvement terms: engagement = 1 - (1 - adequacy) * (1 - human_edit_share) * (1 - steer_share) * (1 - understand_flag).
    ◦ expected_review_ms = REVIEW_FLOOR + REVIEW_RATE * E_lines^0.8 (sublinear).
    ◦ actual_review_ms = clamp(t_next_human_action - t_last_agent_edit, 0, REVIEW_CAP). In auto-accept mode measure at commit time.
    ◦ adequacy = min(1, actual / expected).
    ◦ steer_share is the fraction of turns in the run's episode with act in (specify, steer, decide). understand_flag is 1 if an ask_to_understand turn follows a large diff.
5.  slop = oneshot * agent_share * (1 - engagement). Compute in permille integers (u128, single divide at the end).
6.  Reason codes: EARLY_REVIEW if adequacy < T_ADEQ and E_lines >= N_BIG; HUGE_DIFF if diff_mass_per_run >= N_HUGE; ONESHOT; AGENT_AUTHORED; LOW_ENGAGEMENT; UNVERIFIED (no test run since the last edit or commit while red).
7.  Output the verification modifier separately, never folded into slop. Skip scoring for repos tagged exploratory (still compute, flag it).
B4. Fraud risk tiering (per submission)
Output: tier (pass, sampled_review, hold) plus a finding list. Never an automatic ban.
1.  Each detector emits a finding: {code, layer L1..L4, severity 1..3, measured, threshold, evidence_refs}.
2.  Core detectors and tests:
    ◦ Cadence: over at least 30 inter-prompt gaps, flag if CV < T_CV or if more than P_MODE of gaps fall within 5 percent of the modal gap (metronome).
    ◦ Concurrency: flag if concurrency_ratio > T_CONC (human attention is single-threaded).
    ◦ Throughput: lines_per_credited_hour against the cohort, using the robust z-score z = (x - median) / (1.4826 * MAD). Flag at |z| > 3.5. Cohort = same harness and similar weekly hours, recomputed on a schedule.
    ◦ Activity to commit alignment: credited hours with substantial claimed lines but no commit or push within W_ALIGN, using server-observed push times.
    ◦ Diff plausibility: claimed Edit and Write payloads must be reproducible in the final diff (match rate below T_MATCH is a finding).
    ◦ Review adequacy collapse: many large diffs with adequacy near zero.
    ◦ Integrity (L1): uuid chain valid, timestamps monotonic, valid model and version strings, token usage plausible for elapsed time.
    ◦ Repo provenance: giant initial commit, template fork, commit dedupe hits across the program.
3.  Tier rule, in order:
    ◦ HOLD if any strong link to a banned account (B5), or if findings from at least 2 distinct layers each have severity at least 2, and hours_at_stake >= H_MIN.
    ◦ SAMPLED_REVIEW if any severity 2 or higher finding, or severity sum at least S_SAMPLE.
    ◦ Otherwise PASS, with a random audit at probability p_base + uplift. uplift rises for new accounts (probation), VM or container class, and headless launch. Use a server-secret seed so users cannot predict sampling.
4.  A VM or container class alone never produces a finding, it only changes uplift and the evidence class.
5.  Shadow mode: new detectors log findings without affecting tiers until reviewer outcomes show acceptable precision.
B5. Account linking: ban evaders and farm clusters
Output: link scores between accounts, clusters, and flags for evasion and farming.
1.  Edge types and base weights w:
    ◦ Strong (0.9): same GitHub id or signing key; same commit hash on two accounts; same device hash.
    ◦ Medium (0.3 to 0.5): cross-account prompt SimHash collisions (at least N_COLL pairs within hamming distance 3, prompts longer than the length floor); fork lineage between repos.
    ◦ Weak (0.05 to 0.1, context only): signup burst, same timezone.
2.  Edge damping:
    ◦ Popularity weighting: w_eff = w * log(N / df) normalized, where df is the number of accounts sharing the value. A commit hash or prompt seen in many accounts (templates, tutorials, very common prompts like fix it) gets near-zero weight.
    ◦ Device hashes: ignore placeholders, ignore VM and container class hashes, ignore hashes on chapter-registered shared devices (or divide by account count), and drop any device hash seen on more than M_DEV accounts.
3.  Pair link score by noisy-OR over independent edge types: L(a,b) = 1 - product(1 - w_eff[e]). Treat two edges of the same type as one (take the max) to avoid double counting.
4.  Link if L >= THETA_LINK. Build clusters as connected components over linked pairs, but only chain-merge when the bridging edge is strong or has at least two independent edge types, to prevent giant false clusters. Cap cluster size and send oversized components to manual review.
5.  Ban evasion check on every new or first-upload account: compute L against the banned set. L >= THETA_HOLD gives hold with the exact edges shown to the reviewer. THETA_LINK <= L < THETA_HOLD gives sampled review. New accounts stay on probation (provisional credit) until a clean history accrues.
6.  Farm check at cluster and device level: compute the union of credited intervals across the cluster's accounts. If sum(account credited hours) > T_FARM * union, flag. Also flag hours placed just under per-account caps, accounts taking turns on one device with sequential timing, and accounts per device above K_DEV unless registered.
7.  Behavioral nearest-neighbor matching (cadence histogram, act mix, hour-of-day, harness) is a separate switch, OFF by default, pending the legal answer about monitoring minors (see section on privacy).
B6. Improvement ranking and tip effect
Output: the top 3 patterns with a tip and an example episode each, plus a follow-up result per tip.
1.  For each metric m (yelling rate, retry chain length, correction rate, review adequacy, oneshot rate, time to first green, prompts per commit, session decay): baseline is the median of the last N_BASE sessions (start at 10) and the current window is the last 3 sessions or 7 days. delta_m = (current - baseline) / MAD(baseline).
2.  Cost in minutes: sum the credited minutes in episodes or turns carrying that metric's label or reason code in the current window.
3.  rank_m = cost_minutes * controllability_m * confidence_m, where controllability is a static 0 to 1 table in config (for example context-poor prompting high, task difficulty low). Show the top 3 with delta as supporting evidence, never alone.
4.  For each pattern, pick the episode with the highest contribution as the example, and choose the tip template by reason code.
5.  Session decay knee: compute correction rate by turn-age decile within sessions. The personal restart threshold is the first decile where the rate exceeds 1.5 * the early-session rate for at least 3 sessions.
6.  Tip effect: when a tip is shown, record metric_before (baseline). After N_FOLLOW sessions compute metric_after. Label improved if the shift exceeds 1 MAD in the desired direction, worse if beyond 1 MAD the other way, otherwise no change. Report honestly, including no change, and note task difficulty as a confounder (use the exploratory or routine tag).
B7. Assembly
PESDO per bucket is computed from the factor values per formula.toml (P, E first; S; D conservative or deferred; O gate at submission). The algorithms above feed the factors; this spec does not define the factor expansions because they were never given. Fraud tier and slop are separate outputs, not multiplied into PESDO unless the owner decides otherwise.
