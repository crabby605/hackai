-- Preserve per-user idempotency while preventing cross-account collisions.
ALTER TABLE heartbeats DROP CONSTRAINT IF EXISTS heartbeats_dedupe_key_key;
ALTER TABLE heartbeats ADD CONSTRAINT heartbeats_user_dedupe_key_key UNIQUE (user_id, dedupe_key);

ALTER TABLE submissions ADD CONSTRAINT submissions_replay_key UNIQUE (user_id, project_id, commit_sha, tree_hash);

-- Session bindings follow the session lifetime and are attributable for cleanup.
ALTER TABLE session_ip_bindings ADD COLUMN IF NOT EXISTS user_id BIGINT REFERENCES users(id);
ALTER TABLE session_ip_bindings ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '30 days');
CREATE INDEX IF NOT EXISTS session_ip_bindings_expiry_idx ON session_ip_bindings (expires_at);
