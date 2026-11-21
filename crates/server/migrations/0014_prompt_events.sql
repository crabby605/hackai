CREATE TABLE prompt_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    harness TEXT NOT NULL DEFAULT 'unknown' CHECK (char_length(harness) BETWEEN 1 AND 64),
    session_id TEXT NOT NULL CHECK (char_length(session_id) BETWEEN 1 AND 128),
    occurred_at TIMESTAMPTZ NOT NULL,
    pasted_environment_hint BOOLEAN NOT NULL DEFAULT FALSE,
    tampered_request BOOLEAN NOT NULL DEFAULT FALSE,
    signals JSONB NOT NULL,
    UNIQUE (user_id, project_id, session_id, occurred_at)
);
CREATE INDEX prompt_events_user_project_time_idx ON prompt_events (user_id, project_id, occurred_at DESC);
