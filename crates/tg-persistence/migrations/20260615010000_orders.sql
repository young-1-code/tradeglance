CREATE TABLE orders (
    id               VARCHAR(64) PRIMARY KEY,
    client_order_id  TEXT NOT NULL,
    symbol           VARCHAR(10) NOT NULL,
    exchange         TEXT NOT NULL,
    side             TEXT NOT NULL,
    order_type       TEXT NOT NULL,
    price            NUMERIC(18,4),
    quantity         BIGINT NOT NULL,
    time_in_force    TEXT NOT NULL DEFAULT 'Day',
    strategy_tag     TEXT NOT NULL,
    source           TEXT NOT NULL DEFAULT 'agent',
    status           TEXT NOT NULL,
    filled_quantity  BIGINT NOT NULL DEFAULT 0,
    avg_fill_price   NUMERIC(18,4) NOT NULL DEFAULT 0,
    rejection_reason TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (client_order_id)
);

CREATE INDEX idx_orders_symbol_ts ON orders (symbol, created_at);
CREATE INDEX idx_orders_status ON orders (status) WHERE status IN ('New','PartiallyFilled');
CREATE INDEX idx_orders_strategy ON orders (strategy_tag, created_at);
