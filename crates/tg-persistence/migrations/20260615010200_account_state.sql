CREATE TABLE positions (
    symbol           VARCHAR(10) NOT NULL,
    trading_date     DATE NOT NULL,
    quantity         BIGINT NOT NULL,
    avg_cost         NUMERIC(18,4) NOT NULL,
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (symbol, trading_date)
);

CREATE INDEX idx_positions_symbol ON positions (symbol);

CREATE TABLE accounts (
    snapshot_id      BIGSERIAL PRIMARY KEY,
    ts               TIMESTAMPTZ NOT NULL DEFAULT now(),
    trading_date     DATE NOT NULL,
    cash             NUMERIC(18,4) NOT NULL,
    frozen_cash      NUMERIC(18,4) NOT NULL,
    total_value      NUMERIC(18,4) NOT NULL,
    unrealized_pnl   NUMERIC(18,4) NOT NULL
);

CREATE INDEX idx_accounts_tdate ON accounts (trading_date, ts);
