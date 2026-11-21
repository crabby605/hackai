ALTER TABLE users
    ADD COLUMN IF NOT EXISTS country_code CHAR(2)
    CHECK (country_code IS NULL OR country_code ~ '^[A-Z]{2}$');
