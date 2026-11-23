-- Opt-in, redacted evidence exports. These records are append-only: a bundle is
-- evidence supplied by an untrusted client, not mutable server-side truth.
CREATE TABLE evidence_bundles (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    bundle_id TEXT NOT NULL CHECK (char_length(bundle_id) BETWEEN 1 AND 128),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    processor_version TEXT NOT NULL CHECK (char_length(processor_version) BETWEEN 1 AND 128),
    processor_hash TEXT NOT NULL CHECK (processor_hash ~ '^[0-9a-f]{64}$'),
    formula_version TEXT NOT NULL CHECK (char_length(formula_version) BETWEEN 1 AND 128),
    client_version TEXT NOT NULL CHECK (char_length(client_version) BETWEEN 1 AND 128),
    generated_at TIMESTAMPTZ NOT NULL,
    batch_seq BIGINT,
    prev_batch_hash TEXT CHECK (prev_batch_hash IS NULL OR prev_batch_hash ~ '^[0-9a-f]{64}$'),
    preview_approved BOOLEAN NOT NULL DEFAULT FALSE,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, bundle_id)
);
CREATE INDEX evidence_bundles_user_received_idx ON evidence_bundles (user_id, received_at DESC);

CREATE TABLE evidence_bundle_events (
    id BIGSERIAL PRIMARY KEY,
    bundle_id BIGINT NOT NULL REFERENCES evidence_bundles(id) ON DELETE RESTRICT,
    event_id TEXT NOT NULL CHECK (char_length(event_id) BETWEEN 1 AND 128),
    occurred_at TIMESTAMPTZ NOT NULL,
    event_kind TEXT NOT NULL CHECK (char_length(event_kind) BETWEEN 1 AND 64),
    session_id_hash TEXT NOT NULL CHECK (char_length(session_id_hash) BETWEEN 16 AND 128),
    record JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (bundle_id, event_id)
);
CREATE INDEX evidence_bundle_events_bundle_time_idx ON evidence_bundle_events (bundle_id, occurred_at, id);
