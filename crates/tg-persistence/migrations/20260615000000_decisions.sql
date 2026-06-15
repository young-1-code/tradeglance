CREATE TABLE decisions (
    id               VARCHAR(64) PRIMARY KEY,
    signal_id        VARCHAR(64),
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,
    action           TEXT NOT NULL,
    side             TEXT NOT NULL,
    target_quantity  BIGINT NOT NULL,
    rationale        TEXT NOT NULL,
    risk_checks      JSONB NOT NULL DEFAULT '[]',
    analysis         JSONB,
    pipeline_meta    JSONB,
    source           TEXT NOT NULL,
    ts               TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_decisions_symbol_ts ON decisions (symbol, ts);
CREATE INDEX idx_decisions_source ON decisions (source, ts);
