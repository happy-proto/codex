-- Fork-private data; deliberately outside the upstream migration number sequence.
CREATE TABLE IF NOT EXISTS fork_generation_speeds_v1 (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    turn_id TEXT NOT NULL,
    output_tokens INTEGER NOT NULL CHECK (output_tokens > 0),
    request_duration_us INTEGER NOT NULL CHECK (request_duration_us > 0),
    started_at INTEGER NOT NULL,
    completed_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    PRIMARY KEY (thread_id, turn_id)
);
