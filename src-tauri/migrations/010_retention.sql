-- Value every never-valued observation once with the bounded reach-back rule:
-- one durable keyset job per priced model, handled by its latest version.
INSERT INTO pricing_work(version_id,after_id,through_id)
SELECT p.id,0,(SELECT COALESCE(MAX(id),0) FROM observations) FROM model_price_versions p
WHERE NOT EXISTS(SELECT 1 FROM model_price_versions q WHERE q.model=p.model AND (q.effective_seconds,q.effective_nanos)>(p.effective_seconds,p.effective_nanos))
ON CONFLICT(version_id) DO UPDATE SET after_id=0,through_id=excluded.through_id;
-- Retirement removes a valuation only together with its observation.
DROP TRIGGER IF EXISTS immutable_valuation_delete;
CREATE TRIGGER immutable_valuation_delete BEFORE DELETE ON observation_valuations
WHEN EXISTS(SELECT 1 FROM observations WHERE id=OLD.observation_id)
BEGIN SELECT RAISE(ABORT,'Priced history is immutable'); END;
-- Usage older than the retention floor is never re-imported after retirement.
CREATE TABLE IF NOT EXISTS retention_control (
    id INTEGER PRIMARY KEY CHECK(id=1), floor_seconds INTEGER, floor_nanos INTEGER
);
INSERT OR IGNORE INTO retention_control(id) VALUES(1);
CREATE INDEX IF NOT EXISTS limit_sample_timestamp ON limit_samples(timestamp);
PRAGMA user_version=10;
