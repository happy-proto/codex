CREATE TABLE IF NOT EXISTS fork_ephemeral_live_generation_speeds_v1 (
    thread_id TEXT PRIMARY KEY,
    turn_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    speed REAL CHECK (speed >= 0)
);
