-- Provider-supplied account presentation fields. These are private account
-- fields; public profile queries deliberately do not select them.
-- Migration 0008 (renumbered from 0005 to avoid collision with 0005_annotations.sql).
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS display_name TEXT,
    ADD COLUMN IF NOT EXISTS email TEXT,
    ADD COLUMN IF NOT EXISTS slack_id TEXT,
    ADD COLUMN IF NOT EXISTS avatar_url TEXT;

CREATE INDEX IF NOT EXISTS users_email_idx ON users (email) WHERE email IS NOT NULL;
