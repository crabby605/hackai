-- Profile visibility is private unless a user explicitly opts in to a public page.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS profile_public BOOLEAN NOT NULL DEFAULT TRUE;

-- Profiles are public by default; users can opt out in Settings.
UPDATE users SET profile_public = TRUE WHERE profile_public = FALSE;
