ALTER TABLE productive_episodes DROP CONSTRAINT productive_episodes_attempts_check;
ALTER TABLE productive_episodes ADD CONSTRAINT productive_episodes_attempts_check
    CHECK (attempts >= 0);
