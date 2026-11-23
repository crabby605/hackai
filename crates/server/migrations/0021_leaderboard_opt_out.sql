-- Add leaderboard_opt_out to users table
ALTER TABLE users ADD COLUMN IF NOT EXISTS leaderboard_opt_out BOOLEAN NOT NULL DEFAULT FALSE;
