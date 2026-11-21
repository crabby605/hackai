-- Add Hack Club Auth subject identifier to users.
-- Users are provisioned by OIDC `sub` claim; username defaults to sub until updated.
ALTER TABLE users ADD COLUMN IF NOT EXISTS auth_subject TEXT UNIQUE;
CREATE INDEX IF NOT EXISTS idx_users_auth_subject ON users(auth_subject);
