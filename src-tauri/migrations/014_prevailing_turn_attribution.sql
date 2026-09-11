-- Codex writes no turn_context for turns it starts on its own, such as context
-- compaction. Their usage was stored unattributed, and unattributed usage can
-- never be valued: one such turn leaves the weekly cost-per-percentage-point
-- estimate unavailable for the rest of the cycle. Such a turn still ran under
-- the thread's prevailing settings, so it is attributed from the latest earlier
-- attributed observation of the same thread. Recorded contexts, including a
-- context whose disagreement already erased an attribute, are never overridden.
CREATE INDEX IF NOT EXISTS observation_attribution
    ON observations(thread_id,time_seconds,time_nanos,id)
    WHERE model IS NOT NULL;

PRAGMA user_version=14;
