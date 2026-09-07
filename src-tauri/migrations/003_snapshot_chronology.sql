CREATE INDEX observation_latest ON observations(time_seconds DESC,time_nanos DESC,thread_id ASC)
    WHERE time_seconds IS NOT NULL AND time_nanos IS NOT NULL;
PRAGMA user_version = 3;
