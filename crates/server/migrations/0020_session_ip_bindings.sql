CREATE TABLE IF NOT EXISTS session_ip_bindings (
    token_hash BYTEA PRIMARY KEY,
    client_ip TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
