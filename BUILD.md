# Local Build & Development

## Prerequisites

- **Rust** 1.80+ (`rustup default stable`)
- **Node.js** 20+ with **bun** (`curl -fsSL https://bun.sh/install | bash`)
- **PostgreSQL** 16+ (local or Docker)
- **cargo-fuzz** for parser fuzzing: `cargo install cargo-fuzz`

## One-time setup

```bash
# Clone
git clone https://github.com/crabby605/hackai
cd hackai

# Database (Docker)
docker run --name hackai-db -d \
  -e POSTGRES_USER=hackai \
  -e POSTGRES_PASSWORD=hackai \
  -e POSTGRES_DB=hackai \
  -p 5432:5432 postgres:16

# Or use a local Postgres instance and set DATABASE_URL
```

## Environment

Create `.env` at repo root:

```env
DATABASE_URL=postgres://hackai:hackai@localhost:5432/hackai
HACKAI_SERVER_URL=http://localhost:3000
NEXT_PUBLIC_API_URL=http://localhost:3000
FRONTEND_URL=http://localhost:3001
RUST_LOG=info
```

## Run everything

```bash
# Terminal 1: API server (port 3000)
DATABASE_URL=postgres://hackai:hackai@localhost:5432/hackai \
cargo run -p hackai-server

# Terminal 2: Web dashboard (port 3001)
cd web && NEXT_PUBLIC_API_URL=http://localhost:3000 bun run dev -p 3001
```

## Database migrations

```bash
# Apply migrations
sqlx migrate run

# Create new migration
sqlx migrate add -r descriptive_name
```

## Testing

```bash
# All Rust tests + clippy + fmt
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check

# Parser fuzzing (requires nightly)
cd fuzz && cargo +nightly fuzz run claude_jsonl_parser

# Web type-check + build
cd web && bun run build

# XSS test corpus (33 payloads)
python scripts/load_test.py
python web/tests/xss_test.py
```

## CLI binary

```bash
# Build release
cargo build --release -p hackai

# Binary at ./target/release/hackai
./target/release/hackai --help
```

## Project structure

```
crates/
  cli/          # CLI binary (hackai)
  server/       # Axum API server (hackai-server)
  parser/       # Transcript parsers (Claude, Codex, Gemini, OpenCode, Aider, etc.)
  scorer/       # PESDO scoring formula
  schema/       # Shared types
  gitx/         # Git utilities
  paths/        # Path safety
  schemas/      # SQLx compile-time checked queries

web/
  app/          # Next.js App Router pages
  components/   # React components (charts, UI)
  lib/          # Shared utilities
```

## Common tasks

| Task | Command |
|------|---------|
| Add endpoint | Edit `crates/server/src/main.rs` or modules, register in router, update `docs/API.md` |
| Add DB column | `sqlx migrate add -r name` → write SQL → `sqlx migrate run` → update Rust structs |
| New parser | Add to `crates/parser/src/lib.rs`, add tests, update `crates/cli/src/commands/ingest.rs` |
| Update CLI deps | Edit `crates/cli/Cargo.toml`, run `cargo build` |

## Troubleshooting

- **SQLx offline**: `cargo install sqlx-cli` then `sqlx prepare --workspace`
- **Port conflicts**: Change ports in `.env` and `Caddyfile` (prod)
- **Web build fails**: `cd web && rm -rf .next node_modules && bun install`
- **DB connection**: Verify `DATABASE_URL` and that Postgres accepts local connections

## Production deploy

See `scripts/deploy.sh` and `deploy/` for systemd + Caddy setup on a VPS.