ALTER TABLE commits
    ADD COLUMN committed_at TIMESTAMPTZ,
    ADD COLUMN files_changed INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN lines_added INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN lines_deleted INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN github_observed_at TIMESTAMPTZ;

ALTER TABLE submissions
    ADD COLUMN outcome_factor_permille INTEGER NOT NULL DEFAULT 1000
        CHECK (outcome_factor_permille BETWEEN 0 AND 1000),
    ADD COLUMN github_gate_reason TEXT,
    ADD COLUMN github_observed_at TIMESTAMPTZ;

CREATE INDEX commits_repository_id_idx ON commits(repository_id);
CREATE INDEX submissions_user_project_idx ON submissions(user_id, project_id, submitted_at DESC);
