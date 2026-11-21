-- A project has one repository identity. This backs the ON CONFLICT(project_id)
-- upsert used by sync for clean and migrated databases alike.
CREATE UNIQUE INDEX IF NOT EXISTS repositories_project_id_unique
    ON repositories(project_id);
