-- Advisory tool/lifecycle telemetry. Never used as independent time credit.
CREATE TABLE harness_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL,
    event TEXT NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL,
    tool_category TEXT NOT NULL,
    outcome TEXT NOT NULL,
    UNIQUE (user_id, project_id, harness, session_id, event, occurred_at, tool_category, outcome)
);
CREATE INDEX harness_events_project_time ON harness_events (project_id, occurred_at);
