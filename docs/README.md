# Documentation

See [root README.md](../README.md) for project overview.

## Architecture

- **Rust API** (`crates/server/`) - Axum + PostgreSQL on port 3000
- **Next.js Web** (`web/`) - React 19 + Tailwind on port 3001
- **Database** - PostgreSQL with migrations in `crates/server/migrations/`

## Deployment

- Rust binary: `cargo build --release -p hackai-server`
- Web: `cd web && npm run build`
- Caddy handles HTTPS on 443, proxies to API (3000) and Web (3001)

## Auth

Hack Club OIDC via `HC_AUTH_CLIENT_ID`/`HC_AUTH_CLIENT_SECRET`. Cookie-based sessions with `hackai_session`.