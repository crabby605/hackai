-- Replace plaintext token with hash in cli_pairings for security.
-- The plaintext token is now returned directly in the complete_pairing response
-- over the authenticated TLS channel, never stored at rest.

ALTER TABLE cli_pairings
    ADD COLUMN token_hash BYTEA,
    DROP COLUMN token;