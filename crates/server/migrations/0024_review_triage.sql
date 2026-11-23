-- Evidence-only review queue. Detection opens cases; only an administrator can
-- apply a user-facing enforcement action.
CREATE TABLE review_clusters (
    id BIGSERIAL PRIMARY KEY,
    cluster_key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE review_cases (
    id BIGSERIAL PRIMARY KEY,
    cluster_id BIGINT REFERENCES review_clusters(id),
    subject_user_id BIGINT NOT NULL REFERENCES users(id),
    dedupe_key TEXT NOT NULL UNIQUE,
    priority BIGINT NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'hold', 'sample', 'passed', 'closed')),
    summary TEXT NOT NULL CHECK (char_length(summary) BETWEEN 1 AND 500),
    opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ
);
CREATE INDEX review_cases_queue_idx ON review_cases (status, priority DESC, opened_at ASC);

CREATE TABLE review_case_edges (
    id BIGSERIAL PRIMARY KEY,
    case_id BIGINT NOT NULL REFERENCES review_cases(id) ON DELETE CASCADE,
    edge_type TEXT NOT NULL CHECK (char_length(edge_type) BETWEEN 1 AND 64),
    strength TEXT NOT NULL CHECK (strength IN ('strong', 'medium', 'weak')),
    subject_value TEXT NOT NULL,
    related_user_id BIGINT REFERENCES users(id),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (case_id, edge_type, subject_value, related_user_id)
);

CREATE TABLE review_decisions (
    id BIGSERIAL PRIMARY KEY,
    case_id BIGINT NOT NULL REFERENCES review_cases(id),
    admin_user_id BIGINT NOT NULL REFERENCES users(id),
    decision TEXT NOT NULL CHECK (decision IN ('approve', 'approve_with_note', 'exclude_time', 'request_evidence', 'sample_audit', 'hold', 'mark_cluster')),
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE hardware_fingerprints
    ADD COLUMN environment_class TEXT NOT NULL DEFAULT 'unknown'
    CHECK (environment_class IN ('physical', 'vm', 'container', 'wsl', 'cloud_dev_env', 'unknown'));
