-- One transient snapshot per thread; no upstream migration or protocol changes.
CREATE TABLE IF NOT EXISTS fork_live_generation_speeds_v1 (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    turn_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    run_started_at_ns INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    speed REAL CHECK (speed >= 0)
);
