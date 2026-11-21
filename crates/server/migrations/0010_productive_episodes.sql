CREATE TABLE productive_episodes (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    session_key TEXT NOT NULL CHECK (char_length(session_key) BETWEEN 1 AND 128),
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts >= 1),
    signatures JSONB NOT NULL DEFAULT '[]'::jsonb,
    files_touched JSONB NOT NULL DEFAULT '[]'::jsonb,
    human_edit_seconds BIGINT NOT NULL DEFAULT 0 CHECK (human_edit_seconds >= 0),
    agent_edit_seconds BIGINT NOT NULL DEFAULT 0 CHECK (agent_edit_seconds >= 0),
    resolved BOOLEAN NOT NULL DEFAULT FALSE,
    credited_seconds BIGINT NOT NULL DEFAULT 0 CHECK (credited_seconds >= 0),
    evidence_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    formula_version INTEGER NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, project_id, session_key, started_at)
);

CREATE INDEX productive_episodes_user_project_started_idx
    ON productive_episodes (user_id, project_id, started_at DESC);
