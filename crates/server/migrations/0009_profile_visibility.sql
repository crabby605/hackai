-- Profile visibility is private unless a user explicitly opts in to a public page.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS profile_public BOOLEAN NOT NULL DEFAULT FALSE;
