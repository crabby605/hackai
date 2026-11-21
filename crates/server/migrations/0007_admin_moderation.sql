-- Admin moderation: ban, unban, cooldown modes.
-- Banned users cannot authenticate; cooldown users are rate-limited at ingest.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS banned BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS banned_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS cooldown_until TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_users_banned ON users(banned) WHERE banned;
