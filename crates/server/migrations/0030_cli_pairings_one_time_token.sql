-- The plaintext installation key exists only between browser approval and the
-- first polling CLI request. pairing_status clears it in the same statement.
ALTER TABLE cli_pairings ADD COLUMN IF NOT EXISTS pending_token TEXT;
