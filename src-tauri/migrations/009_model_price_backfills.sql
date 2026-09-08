-- An explicit later backfill records which immutable first price covers older
-- unvalued observations. It never changes a saved price version or valuation.
CREATE TABLE model_price_backfills (
    model TEXT PRIMARY KEY NOT NULL REFERENCES detected_models(model),
    version_id INTEGER NOT NULL REFERENCES model_price_versions(id)
);
CREATE TRIGGER model_price_backfill_matches_model BEFORE INSERT ON model_price_backfills
WHEN (SELECT model FROM model_price_versions WHERE id=NEW.version_id) IS NOT NEW.model
BEGIN SELECT RAISE(ABORT,'Backfill must use the same model price'); END;
CREATE TRIGGER immutable_model_price_backfill_update BEFORE UPDATE ON model_price_backfills
BEGIN SELECT RAISE(ABORT,'Historical price coverage is immutable'); END;
CREATE TRIGGER immutable_model_price_backfill_delete BEFORE DELETE ON model_price_backfills
BEGIN SELECT RAISE(ABORT,'Historical price coverage is immutable'); END;
PRAGMA user_version=9;
