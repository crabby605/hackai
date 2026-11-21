-- Align the local Postgres schema with the serverless D1 schema.
-- Idempotent: safe to re-run; existing columns/types are left untouched.

-- users: BIGSERIAL id, BOOLEAN booleans, TIMESTAMPTZ timestamps, banned/cooldown
-- already match D1 (applied via 0001-0011). Verified, no action needed.

-- productive_episodes: batch_id, harness, present_seconds, pesdo_score_permille,
-- outcome_factor_permille (D1 0005 / 0008). JSON columns are already JSONB locally
-- (0010), so no TEXT -> JSONB conversion is needed here.
ALTER TABLE productive_episodes ADD COLUMN IF NOT EXISTS batch_id TEXT;
ALTER TABLE productive_episodes ADD COLUMN IF NOT EXISTS harness TEXT;
ALTER TABLE productive_episodes ADD COLUMN IF NOT EXISTS present_seconds INTEGER NOT NULL DEFAULT 0;
ALTER TABLE productive_episodes ADD COLUMN IF NOT EXISTS pesdo_score_permille INTEGER;
ALTER TABLE productive_episodes ADD COLUMN IF NOT EXISTS outcome_factor_permille INTEGER NOT NULL DEFAULT 1000;

-- projects: soft-delete timestamp (D1 0006)
ALTER TABLE projects ADD COLUMN IF NOT EXISTS deleted_at TIMESTAMPTZ;

-- repositories: remote_url / head_sha for git remotes (D1 0001; table already exists)
ALTER TABLE repositories ADD COLUMN IF NOT EXISTS remote_url TEXT;
ALTER TABLE repositories ADD COLUMN IF NOT EXISTS head_sha TEXT;

-- installations -> api_keys: api_keys table exists and covers installations;
-- daily_rollups -> rollups: rollups already has human_seconds, agent_seconds,
-- struggle_episodes (0006). Verified, no action needed.
