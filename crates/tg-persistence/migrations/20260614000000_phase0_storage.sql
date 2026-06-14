CREATE TABLE instruments (
    symbol        VARCHAR(10) PRIMARY KEY,
    exchange      TEXT NOT NULL,
    instrument_type TEXT NOT NULL,
    name          TEXT NOT NULL,
    list_date     DATE,
    delist_date   DATE,
    is_st         BOOLEAN NOT NULL DEFAULT false,
    board         TEXT NOT NULL
);

CREATE TABLE watchlist (
    id            BIGSERIAL PRIMARY KEY,
    symbol        VARCHAR(10) NOT NULL REFERENCES instruments(symbol),
    strategy_tags TEXT[] NOT NULL DEFAULT '{}',
    added_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (symbol)
);

CREATE TABLE trading_calendar (
    date          DATE PRIMARY KEY,
    is_trading_day BOOLEAN NOT NULL
);

CREATE TABLE adjustment_factors (
    symbol        VARCHAR(10) NOT NULL,
    ex_date       DATE NOT NULL,
    factor        NUMERIC(18,8) NOT NULL,
    PRIMARY KEY (symbol, ex_date)
);

CREATE TABLE fetch_state (
    symbol           VARCHAR(10) NOT NULL,
    period           TEXT NOT NULL,
    last_fetched_ts  TIMESTAMPTZ,
    last_sync_at     TIMESTAMPTZ,
    status           TEXT NOT NULL,
    last_error       TEXT,
    PRIMARY KEY (symbol, period)
);

CREATE TABLE latest_snapshots (
    symbol        VARCHAR(10) PRIMARY KEY,
    ts            TIMESTAMPTZ NOT NULL,
    trading_date  DATE NOT NULL,
    last          NUMERIC(18,4) NOT NULL,
    open          NUMERIC(18,4) NOT NULL,
    high          NUMERIC(18,4) NOT NULL,
    low           NUMERIC(18,4) NOT NULL,
    pre_close     NUMERIC(18,4) NOT NULL,
    volume        BIGINT NOT NULL,
    amount        NUMERIC(18,4) NOT NULL,
    bid_price     NUMERIC(18,4)[5],
    bid_volume    BIGINT[5],
    ask_price     NUMERIC(18,4)[5],
    ask_volume    BIGINT[5]
);
