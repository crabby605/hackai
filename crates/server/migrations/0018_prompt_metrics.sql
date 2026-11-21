CREATE TABLE IF NOT EXISTS prompt_metrics (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    day_start TIMESTAMPTZ NOT NULL,
    prompt_count BIGINT NOT NULL CHECK (prompt_count > 0),
    proficiency_sum_permille BIGINT NOT NULL CHECK (proficiency_sum_permille >= 0),
    slop_sum_permille BIGINT NOT NULL CHECK (slop_sum_permille >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, project_id, day_start)
);
