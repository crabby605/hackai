-- Tampering status is computed by the server from the deployed SKILL.md SHA.
-- Neither policy text nor raw prompts are uploaded.
CREATE TABLE tampered_sync_requests (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id BIGINT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    observed_policy_sha256 CHAR(64) NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX tampered_sync_requests_project_time_idx
    ON tampered_sync_requests (project_id, received_at DESC);
