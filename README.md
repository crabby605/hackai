# HackAI

HackAI is an AGPLv3 local-first AI coding-time tracker for Hack Club projects.
It extracts features, not prompts, commands, tool outputs, cwd, or absolute
paths.

## Claude Code / agent handoff

Read `CLAUDE_TASKS.md` for the next bounded task, then read
`CLAUDE_HANDOFF.md` before changing code and append a dated entry after each
meaningful change. HackAI is the product name, binary name, and all
user-facing branding. Do not rename it to Hackatime. The one exception is the
locked WakaTime-compatible route:

`POST /api/hackatime/v1/users/current/heartbeats.bulk`

That spelling is protocol compatibility only, never a product name.

## Install the CLI

From a Git repository, install and start the Hack Club device authorization:

```sh
curl -fsSL https://raw.githubusercontent.com/hackclub/hackai/main/scripts/install-cli.sh | bash -s -- --login
```

The installer verifies a published SHA-256 checksum when available and installs
to `~/.local/bin` by default. To authenticate later, run `hackai login`
from the repository; it opens the browser pairing flow automatically. `hackai
login <token>` supports a separately issued API
token for self-hosted deployments.

To update to the latest release, run `hackai update` (prompts before replacing
the binary unless `--yes` is passed). Interactive commands also check once a
day and offer Y/N confirmation. Set `HACKAI_NO_UPDATE_CHECK=1` to disable
checks. Only binaries installed by the release installer are eligible.

Current verified state:

- `schema`, `paths`, `parser`, and `scorer` compile and have focused tests.
- `hackai` CLI has command parsing and local repository-state handling only;
  it is not yet a passive tracking client.
- The Axum server has hardening-layer wiring and migrations, but does **not**
  ingest data yet. Its ingestion routes explicitly return unsupported rather
  than silently dropping or pretending to persist events.

When extending the project, preserve these non-negotiables:

- Never retain prompt text, tool results, cwd, or absolute paths.
- Keep `schema`, `paths`, and `scorer` pure and deterministic; scoring uses
  integer seconds and permille only.
- Resolve/canonicalize paths at the local boundary, then use component-safe
  `strip_prefix`; never split strings on the project name.
- Do not implement Hack Club Auth, Unified Projects DB submission, privacy
  export/deletion behavior, or IP retention policy by guesswork. Their public
  contracts are still required.
- Do not replace an unbuilt integration with a mock or success response.

Run `cargo fmt --all`, `cargo check --workspace`, and the relevant crate tests
after a change. Keep commits small once this directory is initialized as a git
repository.

Parser fuzzing is a first-class check. With nightly Rust and `cargo-fuzz`
installed, run `cargo +nightly fuzz run claude_jsonl_parser` from `fuzz/`.

## Score formula (v1)

Events are grouped into human activity windows. A window earns at most 25
minutes of wall-clock time. Agent-active seconds are multiplied by 300 permille;
error-loop seconds by 1,150 permille; verified human changes to agent output by
1,100 permille. The resulting integer seconds are capped at the wall-clock
window duration. Advisory annotations may only lower or reject a window.

## Threat model

The client treats transcript content, repository metadata, event identifiers,
and all HTTP input as hostile. Prompt/tool-result text is discarded during
parsing. Paths are only emitted after canonical local containment. The service
derives identity from tokens, uses idempotency keys, validates typed fields, and
never fetches a client-provided URL. Public-repository status must be verified
at review time before submitted credit is accepted.

## Known integration gates

- Unified Projects DB API shape and weighted-project calculation.
- Hack Club OIDC issuer/client details and export/deletion requirements.
- Whether anti-fraud IP hashing is permitted and its retention period.

These are deliberately not guessed.
