-- Processed batches table for idempotent sync
-- Prevents duplicate batch processing with a unique constraint

CREATE TABLE IF NOT EXISTS processed_batches (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    batch_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (user_id, batch_id)
);

CREATE INDEX IF NOT EXISTS idx_processed_batches_user_id ON processed_batches (user_id);
CREATE INDEX IF NOT EXISTS idx_processed_batches_created_at ON processed_batches (created_at);