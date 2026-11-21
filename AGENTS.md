# AGENTS.md — HackAI Agent Guidelines

This file tells AI agents how to work in this codebase.

---

## Architecture Overview

**Rust API** (`crates/server/`) — Axum + SQLx + PostgreSQL
- Port 3000, binary `hackai-server`
- All SQL is parameterized (`$1`, `$2`...) — no string interpolation
- Cookie-based auth: `hackai_session` (HttpOnly, Secure, SameSite=Lax)

**Next.js Web** (`web/`) — React 19 + Tailwind v4
- Port 3001, standalone output (`next build && node server.js`)
- Calls Rust API via `fetch(API_BASE + path)` with credentials
- `hackai_session` cookie forwarded automatically

**Database** — PostgreSQL
- Migrations in `crates/server/migrations/`
- `sqlx` compile-time checked queries

---

## Coding Conventions

### Rust
- `#[forbid(unsafe_code)]` at crate root
- `clippy` with `-D warnings` — `unwrap()`, `expect()`, `panic!` denied
- `sqlx::query!()` macros for type-safe SQL
- All user input bound as parameters, never interpolated
- Error types implement `IntoResponse` for Axum

### TypeScript/React
- Server Components by default, Client Components only when needed (`"use client"`)
- `getJson(path)` in `app/lib/api.ts` for server-side data fetching
- `fetch()` with `credentials: "include"` in Client Components
- Tailwind v4 utility classes, no custom CSS modules

### Database
- All timestamps: `TIMESTAMPTZ`
- All IDs: `BIGSERIAL` (not UUID)
- Booleans: `BOOLEAN` (not INTEGER 0/1)
- JSON: `JSONB` columns, queried via `sqlx::types::Json`
- Migrations: `sqlx migrate run` (idempotent, forward-only)

---

## Testing

| Command | Scope |
|---------|-------|
| `cargo test --workspace` | All Rust crates |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint |
| `cargo fmt --all` | Format |
| `cd web && npm run build` | Type-check + build |
| `cd fuzz && cargo fuzz run claude_jsonl_parser` | Fuzz parser |
| `python scripts/load_test.py` | Load/abuse (11 scenarios) |
| `python web/tests/xss_test.py` | XSS corpus (33 payloads) |

---

## Deployment

**Local dev:**
```bash
# Terminal 1: Rust API
DATABASE_URL=postgres://hackai:hackai@localhost:5432/hackai cargo run -p hackai-server

# Terminal 2: Next.js
cd web && NEXT_PUBLIC_API_URL=http://localhost:3000 npm run dev -p 3001
```

**Production:**
- Caddy on :443 → auto-HTTPS → proxies to :3000 (API) and :3001 (Web)
- Systemd services: `hackai-server`, `hackai-web`
- Env vars in `/home/crabby605/hackai/config/env.production`

---

## Key Files to Know

| File | Purpose |
|------|---------|
| `crates/server/src/main.rs` | Router, middleware, core handlers |
| `crates/server/src/auth.rs` | Hack Club OIDC, session cookies |
| `crates/server/src/sync.rs` | CLI sync endpoint |
| `crates/server/src/admin.rs` | Admin endpoints |
| `web/app/lib/api.ts` | Server-side data fetching |
| `web/middleware.ts` | Auth cookie protection |
| `web/app/login/page.tsx` | OIDC login button |
| `docs/API.md` | Complete endpoint reference |

---

## Common Tasks

### Add a new endpoint
1. Add handler in appropriate module (`main.rs`, `admin.rs`, etc.)
2. Register in router in `main.rs`
3. Add SQL with parameterized queries
4. Run `cargo test && cargo clippy`
5. Update `docs/API.md`

### Add a database column
1. Create migration: `sqlx migrate add -r add_column_name`
2. Write `ALTER TABLE ... ADD COLUMN ...`
3. Run `sqlx migrate run`
4. Update Rust structs and SQL queries
5. `cargo test`

### Fix a bug
1. Add test case (Rust: `#[test]`, Fuzz: add corpus)
2. Fix code
3. `cargo test --workspace && cargo clippy`

---

## Security

- **No secrets in code** — all via env vars
- **Parameterized SQL only** — no `format!()` in queries
- **HttpOnly cookies** — no JS access to session
- **CORS restricted** — `localhost:3001` only in dev, domain in prod
- **Rate limits** — Axum `ConcurrencyLimitLayer`, `TimeoutLayer`
- **Input validation** — all user input validated before DB

---

## Don't

- ❌ Use `unwrap()` / `expect()` / `panic!()`
- ❌ Interpolate user input into SQL
- ❌ Store secrets in repo
- ❌ Add `unsafe` blocks
- ❌ Add dependencies without license check (`cargo deny check licenses`)
- ❌ Skip `cargo test` before committing
- ❌ Hardcode URLs — use `NEXT_PUBLIC_API_URL` / `FRONTEND_URL`