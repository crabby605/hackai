-- Per-turn records are strictly derived classifications. This table must never
-- contain raw prompts, replies, source code, paths, tool input/output, or a
-- raw harness session identifier.
CREATE TABLE derived_session_transcript_turns (
    id BIGSERIAL PRIMARY KEY,
    episode_id BIGINT NOT NULL REFERENCES productive_episodes(id) ON DELETE CASCADE,
    turn_index INTEGER NOT NULL CHECK (turn_index >= 0),
    event_id TEXT NOT NULL CHECK (char_length(event_id) BETWEEN 1 AND 256),
    user_act TEXT NOT NULL,
    act_confidence_permille INTEGER NOT NULL CHECK (act_confidence_permille BETWEEN 0 AND 1000),
    new_information TEXT NOT NULL,
    prompt_length_bucket SMALLINT NOT NULL CHECK (prompt_length_bucket BETWEEN 0 AND 255),
    fullcaps BOOLEAN NOT NULL,
    answers_question BOOLEAN NOT NULL,
    agent_state TEXT NOT NULL,
    verification TEXT NOT NULL,
    files_changed_bucket SMALLINT NOT NULL CHECK (files_changed_bucket BETWEEN 0 AND 255),
    tool_errors SMALLINT NOT NULL CHECK (tool_errors BETWEEN 0 AND 255),
    reason_codes JSONB NOT NULL DEFAULT '[]'::jsonb,
    UNIQUE (episode_id, turn_index)
);

CREATE INDEX derived_session_transcript_turns_episode_idx
    ON derived_session_transcript_turns (episode_id, turn_index);
