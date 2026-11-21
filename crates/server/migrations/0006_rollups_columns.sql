-- Add human/agent breakdown and struggle episodes to rollups.
-- Migration 0005 (renumbered from 0004 to avoid collision with 0004_admin_review.sql).
ALTER TABLE rollups ADD COLUMN IF NOT EXISTS human_seconds BIGINT NOT NULL DEFAULT 0;
ALTER TABLE rollups ADD COLUMN IF NOT EXISTS agent_seconds BIGINT NOT NULL DEFAULT 0;
ALTER TABLE rollups ADD COLUMN IF NOT EXISTS struggle_episodes BIGINT NOT NULL DEFAULT 0;
