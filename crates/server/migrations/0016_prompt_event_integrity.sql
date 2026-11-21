ALTER TABLE prompt_events
  ADD COLUMN IF NOT EXISTS harness TEXT NOT NULL DEFAULT 'unknown',
  ADD COLUMN IF NOT EXISTS tampered_request BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE prompt_events
  ADD CONSTRAINT prompt_events_harness_length CHECK (char_length(harness) BETWEEN 1 AND 64);
