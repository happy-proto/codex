-- Numeric-only local channel for ephemeral threads; no thread or conversation row is created.
CREATE TABLE IF NOT EXISTS fork_ephemeral_generation_speeds_v1 (
    thread_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    output_tokens INTEGER NOT NULL CHECK (output_tokens > 0),
    request_duration_us INTEGER NOT NULL CHECK (request_duration_us > 0),
    started_at INTEGER NOT NULL,
    completed_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    PRIMARY KEY (thread_id, turn_id)
);
