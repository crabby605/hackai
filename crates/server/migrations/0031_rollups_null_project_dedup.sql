-- rollups.unique(user_id, project_id, day) treats NULL project_id as distinct,
-- so repeated compute_rollups upserts with project_id = NULL created duplicate
-- (user_id, day) rows. Dedupe them, then enforce uniqueness at the DB layer via
-- a partial unique index (DB constraint over app code).
DELETE FROM rollups r
USING rollups r2
WHERE r.project_id IS NULL
  AND r2.project_id IS NULL
  AND r.user_id = r2.user_id
  AND r.day = r2.day
  AND r.id < r2.id;

CREATE UNIQUE INDEX IF NOT EXISTS rollups_user_day_null_project_unique
  ON rollups (user_id, day) WHERE project_id IS NULL;
