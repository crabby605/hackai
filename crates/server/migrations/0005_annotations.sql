-- Advisory annotation table. Annotations may only lower/flag credit, never raise it.
-- Evidence pointers must verify against transcript events and git or the annotation is discarded.
CREATE TABLE annotations (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    session_key TEXT NOT NULL,
    label TEXT NOT NULL CHECK (label IN ('debugging', 'guided', 'delegated', 'reading', 'exploring')),
    evidence TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    applied BOOLEAN NOT NULL DEFAULT false,
    discount_permille INTEGER NOT NULL DEFAULT 0 CHECK (discount_permille >= 0 AND discount_permille <= 1000)
);

CREATE INDEX idx_annotations_user_session ON annotations(user_id, session_key);
CREATE INDEX idx_annotations_applied ON annotations(applied);
