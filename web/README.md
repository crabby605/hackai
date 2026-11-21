# web — HackAI admin/dashboard WebUI

Day-1 scope scaffold only: a Next.js App Router (TypeScript, strict) admin/dashboard WebUI.

## Why Next.js App Router

SSR for public profiles, ISR for dashboards.

## How to run

```bash
cd web
npm install
npm run dev      # http://localhost:3001 (Next.js dev server)
npm run build    # production build (typechecks as part of the build)
```

## Required environment variables

| Variable | Example | Notes |
|---|---|---|
| `NEXT_PUBLIC_HC_AUTH_CLIENT_ID` | `abc123...` | Hack Club Auth OIDC client id. Used on `/login`. |
| `NEXT_PUBLIC_HC_AUTH_ISSUER` | `https://auth.hackclub.com` | Documented for reference; the authorize endpoint is hard-coded to `https://auth.hackclub.com/oauth/authorize` per the task. |
| `NEXT_PUBLIC_HC_AUTH_REDIRECT_URI` | `http://localhost:3001/auth/callback` | Optional. Defaults to `<origin>/auth/callback`. |
| `HACKAI_INTERNAL_ORIGIN` | `https://hackai.crabby605.workers.dev` | Optional absolute origin used by Server Components for API reads. Browser requests stay same-origin. |

For deployments where the API and web app use different subdomains, set `SESSION_COOKIE_DOMAIN` on the server to their shared parent domain (for example, `.example.com`). This lets the persistent, HttpOnly session cookie remain available to the SSR web app until its expiry. Leave it unset when both services share one host or during local development.

Set these in `.env` (Next.js loads `.env` automatically; `NEXT_PUBLIC_*` are exposed to the browser).

## Routes

- `/` — Dashboard: hours/day, human vs agent ratio, struggle episodes. Reads from the server; no data yet → empty states. No mock data.
- `/profile/[username]` — Public profile (SSR). Empty/not-found state when the user has no data.
- `/login` — Hack Club Auth OIDC login button linking to `https://auth.hackclub.com/oauth/authorize` with `client_id` from `NEXT_PUBLIC_HC_AUTH_CLIENT_ID`. Shows a config error when the client id is missing.

## Server integration

The dashboard is moving to the Cloudflare Worker in `serverless/`. The former
Rust/Axum server source remains available for reference but is excluded from
normal Rust workspace builds; it is not a deployed binary.
