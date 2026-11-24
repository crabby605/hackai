# HackAI change log

This is the human-readable record of changes that affect users, deployments,
data collection, or the public API. Add an entry here in the same change that
ships it; do not rely on commit history as release notes.

## Unreleased

### Fixed

- CLI browser pairing now transfers the one-time installation credential to
  the polling CLI atomically, instead of consuming an approved pairing before
  the token could be returned.
- Project sync can use the local directory name for non-Git projects.
- Project summary aggregate queries handle PostgreSQL `SUM` values correctly.
- Browser authentication consistently uses the `__Host-hackai_session` cookie.

### Changed

- The CLI pairing handoff adds `cli_pairings.pending_token`. It is cleared when
  the CLI claims it; the durable API-key hash remains the only long-lived
  credential stored by the server.
- OIDC requests the Hack Club Auth `slack_id` scope and uses Cachet for an
  avatar when the identity provider has no picture claim.

### Deployment checklist

- Build API and web artifacts locally; do not build application artifacts on
  production.
- Run the relevant tests and `cargo clippy -- -D warnings` before deployment.
- Apply migrations before relying on a schema change, restart only the changed
  service, then health-check the local listener and public route.
- Record user-visible, security, collection, schema, and configuration changes
  in this file.
