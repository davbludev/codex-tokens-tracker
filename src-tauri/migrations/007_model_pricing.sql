CREATE TABLE detected_models (
    model TEXT PRIMARY KEY NOT NULL CHECK(length(trim(model)) > 0)
);
INSERT OR IGNORE INTO detected_models(model)
SELECT model FROM turn_contexts WHERE model IS NOT NULL AND length(trim(model)) > 0;
INSERT OR IGNORE INTO detected_models(model)
SELECT model FROM observations WHERE model IS NOT NULL AND length(trim(model)) > 0;

CREATE TABLE model_price_versions (
    id INTEGER PRIMARY KEY,
    model TEXT NOT NULL REFERENCES detected_models(model),
    effective_seconds INTEGER NOT NULL,
    effective_nanos INTEGER NOT NULL CHECK(effective_nanos BETWEEN 0 AND 999999999),
    backfill_before INTEGER NOT NULL CHECK(backfill_before IN (0,1)),
    configuration TEXT NOT NULL,
    UNIQUE(model,effective_seconds,effective_nanos)
);
CREATE TABLE observation_valuations (
    observation_id INTEGER PRIMARY KEY REFERENCES observations(id),
    version_id INTEGER NOT NULL REFERENCES model_price_versions(id),
    model TEXT NOT NULL,
    amount TEXT NOT NULL
);
-- Application writes only insert valuations and versions. Guard accidental
-- future update paths as well as preserving them through replay.
CREATE TRIGGER immutable_valuation_update BEFORE UPDATE ON observation_valuations
BEGIN SELECT RAISE(ABORT,'Priced history is immutable'); END;
CREATE TRIGGER immutable_valuation_delete BEFORE DELETE ON observation_valuations
BEGIN SELECT RAISE(ABORT,'Priced history is immutable'); END;
CREATE TRIGGER immutable_price_update BEFORE UPDATE ON model_price_versions
BEGIN SELECT RAISE(ABORT,'Price versions are immutable'); END;
CREATE TRIGGER immutable_price_delete BEFORE DELETE ON model_price_versions
BEGIN SELECT RAISE(ABORT,'Price versions are immutable'); END;

-- One bounded keyset pass per saved version. The watermark excludes later
-- arrivals, which are valued directly when accepted.
CREATE TABLE pricing_work (
    version_id INTEGER PRIMARY KEY REFERENCES model_price_versions(id),
    after_id INTEGER NOT NULL DEFAULT 0,
    through_id INTEGER NOT NULL
);
CREATE INDEX observation_pricing_model ON observations(model,id) WHERE accepted=1;
PRAGMA user_version=7;
