-- A reversible staff-only leaderboard exclusion. Unlike a ban, this does not
-- block login or syncing; it only removes the account from public rankings.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS shadow_banned BOOLEAN NOT NULL DEFAULT FALSE;
