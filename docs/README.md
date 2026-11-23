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

### Docker Compose

Docker Compose runs PostgreSQL, a one-shot migration job, the API, the web
server, and Caddy. Caddy obtains TLS certificates for `DOMAIN` automatically.

```sh
cp .env.example .env
# Edit .env: set DOMAIN, POSTGRES_PASSWORD, and Hack Club OIDC credentials.
docker compose up -d --build
docker compose ps
docker compose logs -f migrate api web caddy
```

The `migrate` service must complete successfully before the API starts. For a
subsequent release, pull or copy the new source and run `docker compose up -d
--build`; the migration job is safe to re-run. PostgreSQL data is kept in the
named `postgres-data` volume. Back it up before a major upgrade:

```sh
docker compose exec -T db pg_dump -U "$POSTGRES_USER" "$POSTGRES_DB" > hackai.sql
```

## Auth

Hack Club OIDC via `HC_AUTH_CLIENT_ID`/`HC_AUTH_CLIENT_SECRET`. Cookie-based sessions with `hackai_session`.
