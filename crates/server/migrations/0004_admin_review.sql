-- The `role` column was removed; `is_admin` from 0003_add_is_admin.sql is the
-- single source of truth for administrator status. Keeping one column avoids
-- divergence between role and is_admin.

CREATE TABLE session_integrity_reviews (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    session_key TEXT NOT NULL CHECK (char_length(session_key) BETWEEN 1 AND 128),
    real_seconds BIGINT NOT NULL CHECK (real_seconds >= 0),
    reported_seconds BIGINT CHECK (reported_seconds >= 0),
    delta_seconds BIGINT GENERATED ALWAYS AS (reported_seconds - real_seconds) STORED,
    status TEXT NOT NULL DEFAULT 'unreviewed'
        CHECK (status IN ('unreviewed', 'reviewed', 'dismissed')),
    evidence JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reviewed_at TIMESTAMPTZ,
    reviewed_by BIGINT REFERENCES users(id),
    UNIQUE (user_id, session_key)
);

CREATE INDEX session_integrity_reviews_status_created_idx
    ON session_integrity_reviews (status, created_at DESC);

CREATE TABLE admin_audit_log (
    id BIGSERIAL PRIMARY KEY,
    admin_user_id BIGINT NOT NULL REFERENCES users(id),
    action TEXT NOT NULL CHECK (char_length(action) BETWEEN 1 AND 64),
    target_user_id BIGINT REFERENCES users(id),
    target_session_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX admin_audit_log_admin_created_idx
    ON admin_audit_log (admin_user_id, created_at DESC);
