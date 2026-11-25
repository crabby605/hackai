-- Explicitly opted-in, short agent coaching notes. Raw transcripts and prompt
-- content remain outside this table and are never accepted by sync.
CREATE TABLE IF NOT EXISTS user_feedback (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    recorded_at TIMESTAMPTZ NOT NULL,
    feedback TEXT NOT NULL CHECK (char_length(feedback) BETWEEN 1 AND 600),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, project_id, recorded_at)
);

CREATE INDEX IF NOT EXISTS user_feedback_project_time_idx
    ON user_feedback (project_id, recorded_at DESC);
