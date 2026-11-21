-- Local-development demo data. Every account is explicitly prefixed `demo-`.
-- Apply only to a disposable/local `hackai` database.
BEGIN;

INSERT INTO users (username, auth_subject, display_name, profile_public)
VALUES
  ('demo-ada', 'demo-user-ada', 'Ada Rivera', TRUE),
  ('demo-sam', 'demo-user-sam', 'Sam Chen', TRUE),
  ('demo-jules', 'demo-user-jules', 'Jules Okafor', FALSE)
ON CONFLICT (auth_subject) DO UPDATE
SET username = EXCLUDED.username, display_name = EXCLUDED.display_name, profile_public = EXCLUDED.profile_public;

INSERT INTO projects (name, owner_id, private)
SELECT project_name, users.id, FALSE
FROM (VALUES
  ('demo-ada', 'orbit-ui'),
  ('demo-ada', 'signal-parser'),
  ('demo-sam', 'lumen-api'),
  ('demo-jules', 'pixel-garden')
) AS demo(owner, project_name)
JOIN users ON users.username = demo.owner
ON CONFLICT (owner_id, name) DO NOTHING;

INSERT INTO repositories (project_id, remote_owner, remote_name, public)
SELECT projects.id, 'hackai-demo', projects.name, TRUE
FROM projects
JOIN users ON users.id = projects.owner_id
WHERE users.username LIKE 'demo-%'
ON CONFLICT (project_id) DO NOTHING;

INSERT INTO rollups (user_id, project_id, day, credited_seconds, human_seconds, agent_seconds, struggle_episodes)
SELECT users.id, projects.id, CURRENT_DATE - offsets.day_offset, offsets.credited_seconds,
       offsets.human_seconds, offsets.agent_seconds, offsets.episodes
FROM (VALUES
  ('demo-ada', 'orbit-ui', 0, 14400::BIGINT, 10800::BIGINT, 3600::BIGINT, 2),
  ('demo-ada', 'signal-parser', 1, 10800::BIGINT, 8400::BIGINT, 2400::BIGINT, 1),
  ('demo-sam', 'lumen-api', 0, 12600::BIGINT, 7200::BIGINT, 5400::BIGINT, 3),
  ('demo-jules', 'pixel-garden', 2, 9000::BIGINT, 6900::BIGINT, 2100::BIGINT, 1)
) AS offsets(owner, project_name, day_offset, credited_seconds, human_seconds, agent_seconds, episodes)
JOIN users ON users.username = offsets.owner
JOIN projects ON projects.owner_id = users.id AND projects.name = offsets.project_name
ON CONFLICT (user_id, project_id, day) DO UPDATE SET
  credited_seconds = EXCLUDED.credited_seconds,
  human_seconds = EXCLUDED.human_seconds,
  agent_seconds = EXCLUDED.agent_seconds,
  struggle_episodes = EXCLUDED.struggle_episodes;

INSERT INTO productive_episodes
  (user_id, project_id, session_key, started_at, ended_at, attempts, signatures, files_touched,
   human_edit_seconds, agent_edit_seconds, resolved, credited_seconds, evidence_ids, formula_version)
SELECT users.id, projects.id, demo.session_key, NOW() - demo.started_ago, NOW() - demo.ended_ago,
       demo.attempts, demo.signatures::JSONB, demo.files::JSONB, demo.human_edit_seconds,
       demo.agent_edit_seconds, demo.resolved, demo.credited_seconds, demo.evidence::JSONB, 1
FROM (VALUES
  ('demo-ada', 'orbit-ui', 'demo-orbit-1', interval '4 hours', interval '3 hours 25 minutes', 4,
   '["a1b2c3d4e5f60123456789abcdef0123456789abcdef0123456789abcdef0123"]', '["src/routes/editor.tsx"]',
   930::BIGINT, 180::BIGINT, TRUE, 870::BIGINT, '["demo-event-1","demo-event-2"]'),
  ('demo-sam', 'lumen-api', 'demo-lumen-1', interval '2 hours', interval '1 hour 5 minutes', 6,
   '["b1b2c3d4e5f60123456789abcdef0123456789abcdef0123456789abcdef0123"]', '["src/auth/session.rs","src/auth/token.rs"]',
   1400::BIGINT, 260::BIGINT, TRUE, 1260::BIGINT, '["demo-event-3","demo-event-4"]')
) AS demo(owner, project_name, session_key, started_ago, ended_ago, attempts, signatures, files, human_edit_seconds, agent_edit_seconds, resolved, credited_seconds, evidence)
JOIN users ON users.username = demo.owner
JOIN projects ON projects.owner_id = users.id AND projects.name = demo.project_name
ON CONFLICT (user_id, project_id, session_key, started_at) DO NOTHING;

COMMIT;
