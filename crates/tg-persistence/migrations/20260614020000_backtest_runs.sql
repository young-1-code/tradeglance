CREATE TABLE backtest_runs (
    id          TEXT PRIMARY KEY,
    strategy    TEXT NOT NULL,
    symbols     TEXT[] NOT NULL,
    config      JSONB NOT NULL,
    status      TEXT NOT NULL,
    metrics     JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);
