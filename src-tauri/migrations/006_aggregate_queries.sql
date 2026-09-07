-- Read-side indexes only: accepted deltas and effective hierarchy remain authoritative.
CREATE INDEX session_effective_children ON sessions(parent_thread_id, parent_state, thread_id);
CREATE INDEX observation_model_bucket ON observations(
    CASE WHEN model IS NULL THEN 'unknown:' ELSE 'model:' || model END,
    thread_id, accepted
);
PRAGMA user_version = 6;
