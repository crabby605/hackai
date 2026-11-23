# HackAI API Documentation

## Base URL
- **Development**: `http://localhost:3000`
- **Production**: `https://hackclub.app`

## Authentication
All authenticated endpoints require the `hackai_session` cookie (HttpOnly, Secure, SameSite=Lax).
Set via `/auth/callback` after Hack Club OIDC login.

## Endpoints

### Auth

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/auth/login` | Public | Start Hack Club OIDC login. Query: `continue=/path` |
| GET | `/auth/callback` | Public | OIDC callback, sets session cookie |
| GET | `/healthz` | Public | Health check |

### User

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/me` | Session | Current user profile |
| POST | `/api/v1/me/profile` | Session | Update profile (username, profile_public, country_code) |
| GET | `/api/v1/me/username-availability/{username}` | Session | Check if username available |

### Dashboard

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/dashboard/stats` | Session | Daily stats (hours, human/agent ratio, episodes) |
| GET | `/api/v1/dashboard/insights` | Session | Insights (harness, language, active day, PESDO) |
| GET | `/api/v1/coach` | Session | Privacy-safe coaching patterns from opt-in derived evidence; never returns prompt text |
| GET | `/api/v1/risk-findings` | Session | Transparent, review-only derived-evidence findings; never changes score, eligibility, or account status |

### Projects

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/projects` | Session | List user's projects |
| DELETE | `/api/v1/projects/{id}` | Session | Soft-delete project |
| POST | `/api/v1/projects/merge` | Session | Merge source into target project |

### Episodes

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/episodes` | Session | List recent episodes (100) |
| GET | `/api/v1/struggles` | Session | Struggle days with human/agent/struggle breakdown |

### Leaderboard

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/leaderboard` | Public | Top 100 users by credited seconds |

### Public Profiles

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/users/{username}` | Public | Public profile (username, display_name, avatar, country) |

### Admin (requires `is_admin=true`)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/admin/insights` | Admin | System-wide stats |
| GET | `/api/v1/admin/users` | Admin | List all users |
| GET | `/api/v1/admin/users/{id}` | Admin | User detail (projects, episodes, installations) |
| GET | `/api/v1/admin/users/{id}/projects` | Admin | User's projects |
| GET | `/api/v1/admin/activity` | Admin | Recent activity feed |
| POST | `/api/v1/admin/users/{id}/ban` | Admin | Ban user |
| POST | `/api/v1/admin/users/{id}/unban` | Admin | Unban user |
| POST | `/api/v1/admin/users/{id}/cooldown` | Admin | Set ingest cooldown |
| POST | `/api/v1/admin/hardware-ban` | Admin | Ban all users matching a SHA-256 hardware digest |

### CLI Pairing (OAuth 2.0 Device Flow)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/v1/cli/pairings` | Public | Start pairing (returns device_code, user_code) |
| POST | `/api/v1/cli/pairings/complete` | Session | Approve pairing (body: `{code}`) |
| GET | `/api/v1/cli/pairings/{code}` | Public | Poll for token (returns token once) |

### Sync (CLI)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/api/v1/sync` | Bearer token | Push episodes, annotations, checkpoints, and optional SHA-256 hardware digests |
| POST | `/api/v1/evidence-bundles` | Session or Bearer token | Opt-in redacted event-level evidence bundle; raw prompt text, code, paths, and tool output are rejected |
| GET | `/api/v1/evidence-bundles` | Session or Bearer token | Caller’s redacted evidence-bundle provenance summaries |
| GET | `/api/v1/evidence-bundles/{bundle_id}` | Session or Bearer token | Caller’s derived event records and provenance |
| GET | `/api/v1/admin/users/{id}/evidence-bundles` | Admin | Reviewer view of a user’s redacted provenance summaries |
| GET | `/api/v1/admin/users/{id}/evidence-bundles/{bundle_id}` | Admin | Reviewer view of a redacted evidence packet |
| GET | `/api/v1/admin/users/{id}/risk-findings` | Admin | Conservative review-only findings from that user’s derived evidence; no automatic enforcement |

### SSE

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/v1/sse` | Session | Server-Sent Events stream |

### Native Events (WakaTime compatible)

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/v1/events` | Bearer token | Native event ingest |
| POST | `/api/hackatime/v1/users/current/heartbeats.bulk` | Bearer token | WakaTime heartbeat bulk |

---

## Sync Payload

```json
{
  "remote_url": "https://github.com/owner/repo.git",
  "head_sha": "abc123...",
  "annotations": [
    {"session_key": "...", "label": "debugging", "evidence": "...", "applied": false, "discount_permille": 0}
  ],
  "checkpoints": [{"cursor": "..."}],
  "episodes": [
    {
      "formula_version": 1,
      "started_at": 1699999999000,
      "ended_at": 1700000000000,
      "session": "session-key",
      "attempts": 3,
      "distinct_signatures": ["sha256..."],
      "files_touched": ["src/main.rs"],
      "human_edit_seconds": 1800,
      "agent_edit_seconds": 7200,
      "resolved": true,
      "credited_seconds": 5400,
      "buckets": [{"evidence_ids": ["evt-1", "evt-2"]}]
    }
  ]
}
```

---

## Response Format

Success: `{ "status": "synced", "project_id": 1, ... }`

Error: `{ "error": "message" }` with appropriate HTTP status.

---

## Rate Limits

- Auth endpoints: 10/min
- API endpoints: 100/min per user
- Sync: 20/min per installation
