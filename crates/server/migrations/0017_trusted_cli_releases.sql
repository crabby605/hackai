-- Release hashes are registered by an administrator during deployment.
-- Uploads from unknown builds are retained but marked tampered_request.
CREATE TABLE trusted_cli_releases (
    sha256 CHAR(64) PRIMARY KEY,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
