-- Reasoning effort travels with a turn's model. Existing turns keep NULL: the
-- effort was never recorded and history is never re-imported to invent it.
ALTER TABLE turn_contexts ADD COLUMN effort TEXT;
ALTER TABLE observations ADD COLUMN effort TEXT;
PRAGMA user_version=11;
