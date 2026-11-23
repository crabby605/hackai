-- Hardware identifiers are SHA-256 digests supplied by the CLI; raw values are
-- never accepted or stored.
CREATE TABLE hardware_fingerprints (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL CHECK (kind IN ('fingerprint', 'asset_tag', 'serial_number')),
    sha256_hash CHAR(64) NOT NULL CHECK (sha256_hash ~ '^[0-9a-f]{64}$'),
    first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, kind, sha256_hash)
);
CREATE INDEX hardware_fingerprints_hash_idx ON hardware_fingerprints (sha256_hash);

CREATE TABLE hardware_bans (
    sha256_hash CHAR(64) PRIMARY KEY CHECK (sha256_hash ~ '^[0-9a-f]{64}$'),
    banned_by BIGINT NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
