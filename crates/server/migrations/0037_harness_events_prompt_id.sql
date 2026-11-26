-- Add prompt_id to correlate harness events with their triggering prompt.
ALTER TABLE harness_events ADD COLUMN prompt_id TEXT;
CREATE INDEX harness_events_prompt_id ON harness_events (prompt_id) WHERE prompt_id IS NOT NULL;