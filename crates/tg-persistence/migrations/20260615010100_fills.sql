CREATE TABLE fills (
    fill_id          VARCHAR(64) PRIMARY KEY,
    order_id         VARCHAR(64) NOT NULL REFERENCES orders(id),
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,
    side             TEXT NOT NULL,
    price            NUMERIC(18,4) NOT NULL,
    quantity         BIGINT NOT NULL,
    commission       NUMERIC(18,4) NOT NULL,
    tax              NUMERIC(18,4) NOT NULL,
    transfer_fee     NUMERIC(18,4) NOT NULL,
    ts               TIMESTAMPTZ NOT NULL DEFAULT now(),
    trading_date     DATE NOT NULL
);

CREATE INDEX idx_fills_order ON fills (order_id);
CREATE INDEX idx_fills_symbol_ts ON fills (symbol, ts);
CREATE INDEX idx_fills_tdate ON fills (trading_date);
