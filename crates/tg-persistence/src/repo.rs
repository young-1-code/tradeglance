use std::path::PathBuf;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json;
use sqlx::{PgPool, Row};
use tg_contracts::{
    Account, Adjustment, AdjustmentFactor, Bar, BarQuery, Decision, DecisionAction, Fill,
    Instrument, Order, OrderSide, OrderStatus, OrderType, Result, RiskCheckResult, Snapshot,
    StrategyStyle, TgError, TimeInForce, TradingCalendar,
};

use crate::adjust::adjust_bars;
use crate::model::{
    board_from_str, board_to_str, exchange_from_str, exchange_to_str, fixed_5,
    instrument_type_from_str, instrument_type_to_str, other_error,
};
use crate::parquet_io::ParquetStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistEntry {
    pub symbol: String,
    pub strategy_tags: Vec<String>,
    pub added_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BacktestRunRecord {
    pub id: String,
    pub strategy: String,
    pub symbols: Vec<String>,
    pub config: Value,
    pub status: String,
    pub metrics: Option<Value>,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecisionAuditRecord {
    pub decision: Decision,
    pub analysis: Option<Value>,
    pub pipeline_meta: Option<Value>,
    pub source: String,
}

#[async_trait]
pub trait BarRepo: Send + Sync {
    async fn write_bars(&self, bars: &[Bar]) -> Result<()>;
    async fn query_bars(&self, q: BarQuery) -> Result<Vec<Bar>>;
}

#[async_trait]
pub trait SnapshotRepo: Send + Sync {
    async fn write_snapshot(&self, s: &Snapshot) -> Result<()>;
    async fn get_latest(&self, symbol: &str) -> Result<Option<Snapshot>>;
}

#[async_trait]
pub trait InstrumentRepo: Send + Sync {
    async fn upsert_instrument(&self, instrument: &Instrument) -> Result<()>;
    async fn list_instruments(&self) -> Result<Vec<Instrument>>;
    async fn get_instrument(&self, symbol: &str) -> Result<Option<Instrument>>;
    async fn add_to_watchlist(&self, symbol: &str, strategy_tags: &[String]) -> Result<()>;
    async fn remove_from_watchlist(&self, symbol: &str) -> Result<()>;
    async fn list_watchlist(&self) -> Result<Vec<WatchlistEntry>>;
}

#[async_trait]
pub trait CalendarRepo: Send + Sync {
    async fn upsert_calendar(&self, days: &[TradingCalendar]) -> Result<()>;
    async fn is_trading_day(&self, date: NaiveDate) -> Result<bool>;
    async fn calendar_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<TradingCalendar>>;
}

#[async_trait]
pub trait FactorRepo: Send + Sync {
    async fn upsert_adjustment_factors(&self, factors: &[AdjustmentFactor]) -> Result<()>;
    async fn query_adjustment_factors(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<AdjustmentFactor>>;
}

#[async_trait]
pub trait BacktestRunRepo: Send + Sync {
    async fn save_run(&self, run: BacktestRunRecord) -> Result<()>;
    async fn get_run(&self, id: &str) -> Result<Option<BacktestRunRecord>>;
    async fn list_runs(&self) -> Result<Vec<BacktestRunRecord>>;
}

#[async_trait]
pub trait DecisionRepo: Send + Sync {
    async fn save_decision(&self, record: DecisionAuditRecord) -> Result<()>;
    async fn list_decisions(
        &self,
        symbol: Option<&str>,
        limit: i64,
    ) -> Result<Vec<DecisionAuditRecord>>;
}

#[async_trait]
pub trait OrderRepo: Send + Sync {
    async fn save_order(&self, order: &Order) -> Result<()>;
    async fn get_order(&self, id: &str) -> Result<Option<Order>>;
    async fn list_open_orders(&self) -> Result<Vec<Order>>;
}

#[async_trait]
pub trait FillRepo: Send + Sync {
    async fn save_fill(&self, fill: &Fill) -> Result<()>;
    async fn list_fills(&self, order_id: Option<&str>, limit: i64) -> Result<Vec<Fill>>;
}

#[async_trait]
pub trait AccountStateRepo: Send + Sync {
    async fn save_account(&self, account: &Account, trading_date: NaiveDate) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct PostgresStore {
    pool: PgPool,
    parquet: ParquetStore,
}

impl PostgresStore {
    pub async fn connect(database_url: &str, data_root: impl Into<PathBuf>) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(database_url)
            .await
            .map_err(other_error)?;
        Ok(Self::from_pool(pool, data_root))
    }

    pub fn from_pool(pool: PgPool, data_root: impl Into<PathBuf>) -> Self {
        Self {
            pool,
            parquet: ParquetStore::new(data_root),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn parquet(&self) -> &ParquetStore {
        &self.parquet
    }

    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(other_error)
    }

    async fn query_adjustment_factors_until(
        &self,
        symbol: &str,
        end: NaiveDate,
    ) -> Result<Vec<AdjustmentFactor>> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, ex_date, factor
            FROM adjustment_factors
            WHERE symbol = $1 AND ex_date <= $2
            ORDER BY ex_date
            "#,
        )
        .bind(symbol)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(adjustment_factor_from_row).collect()
    }
}

#[async_trait]
impl BarRepo for PostgresStore {
    async fn write_bars(&self, bars: &[Bar]) -> Result<()> {
        self.parquet.write_bars(bars).await
    }

    async fn query_bars(&self, q: BarQuery) -> Result<Vec<Bar>> {
        let adjustment = q.adjustment;
        let symbol = q.symbol.clone();
        let end_date = q.range.end.date_naive();
        let mut bars = self.parquet.query_bars(&q).await?;

        if !matches!(adjustment, Adjustment::None) {
            let factors = self
                .query_adjustment_factors_until(&symbol, end_date)
                .await?;
            bars = adjust_bars(bars, &factors, adjustment)?;
        }

        Ok(bars)
    }
}

#[async_trait]
impl SnapshotRepo for PostgresStore {
    async fn write_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        self.parquet.write_snapshot(snapshot).await?;
        upsert_latest_snapshot(&self.pool, snapshot).await
    }

    async fn get_latest(&self, symbol: &str) -> Result<Option<Snapshot>> {
        let row = sqlx::query(
            r#"
            SELECT s.symbol, i.exchange, s.ts, s.trading_date, s.last, s.open, s.high,
                   s.low, s.pre_close, s.volume, s.amount, s.bid_price, s.bid_volume,
                   s.ask_price, s.ask_volume
            FROM latest_snapshots s
            LEFT JOIN instruments i ON i.symbol = s.symbol
            WHERE s.symbol = $1
            "#,
        )
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await
        .map_err(other_error)?;

        row.map(snapshot_from_latest_row).transpose()
    }
}

#[async_trait]
impl InstrumentRepo for PostgresStore {
    async fn upsert_instrument(&self, instrument: &Instrument) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO instruments (
                symbol, exchange, instrument_type, name, list_date, delist_date, is_st, board
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (symbol) DO UPDATE SET
                exchange = EXCLUDED.exchange,
                instrument_type = EXCLUDED.instrument_type,
                name = EXCLUDED.name,
                list_date = EXCLUDED.list_date,
                delist_date = EXCLUDED.delist_date,
                is_st = EXCLUDED.is_st,
                board = EXCLUDED.board
            "#,
        )
        .bind(&instrument.symbol)
        .bind(exchange_to_str(instrument.exchange))
        .bind(instrument_type_to_str(instrument.instrument_type))
        .bind(&instrument.name)
        .bind(instrument.list_date)
        .bind(instrument.delist_date)
        .bind(instrument.is_st)
        .bind(board_to_str(instrument.board))
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn list_instruments(&self) -> Result<Vec<Instrument>> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, exchange, instrument_type, name, list_date, delist_date, is_st, board
            FROM instruments
            ORDER BY symbol
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(instrument_from_row).collect()
    }

    async fn get_instrument(&self, symbol: &str) -> Result<Option<Instrument>> {
        let row = sqlx::query(
            r#"
            SELECT symbol, exchange, instrument_type, name, list_date, delist_date, is_st, board
            FROM instruments
            WHERE symbol = $1
            "#,
        )
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await
        .map_err(other_error)?;

        row.map(instrument_from_row).transpose()
    }

    async fn add_to_watchlist(&self, symbol: &str, strategy_tags: &[String]) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO watchlist (symbol, strategy_tags)
            VALUES ($1, $2)
            ON CONFLICT (symbol) DO UPDATE SET
                strategy_tags = EXCLUDED.strategy_tags
            "#,
        )
        .bind(symbol)
        .bind(strategy_tags)
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn remove_from_watchlist(&self, symbol: &str) -> Result<()> {
        sqlx::query("DELETE FROM watchlist WHERE symbol = $1")
            .bind(symbol)
            .execute(&self.pool)
            .await
            .map_err(other_error)?;
        Ok(())
    }

    async fn list_watchlist(&self) -> Result<Vec<WatchlistEntry>> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, strategy_tags, added_at
            FROM watchlist
            ORDER BY symbol
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(watchlist_from_row).collect()
    }
}

#[async_trait]
impl CalendarRepo for PostgresStore {
    async fn upsert_calendar(&self, days: &[TradingCalendar]) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(other_error)?;
        for day in days {
            sqlx::query(
                r#"
                INSERT INTO trading_calendar (date, is_trading_day)
                VALUES ($1, $2)
                ON CONFLICT (date) DO UPDATE SET
                    is_trading_day = EXCLUDED.is_trading_day
                "#,
            )
            .bind(day.date)
            .bind(day.is_trading_day)
            .execute(&mut *tx)
            .await
            .map_err(other_error)?;
        }
        tx.commit().await.map_err(other_error)
    }

    async fn is_trading_day(&self, date: NaiveDate) -> Result<bool> {
        let row = sqlx::query(
            r#"
            SELECT is_trading_day
            FROM trading_calendar
            WHERE date = $1
            "#,
        )
        .bind(date)
        .fetch_optional(&self.pool)
        .await
        .map_err(other_error)?;

        Ok(row
            .map(|row| row.try_get::<bool, _>("is_trading_day"))
            .transpose()
            .map_err(other_error)?
            .unwrap_or(false))
    }

    async fn calendar_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<TradingCalendar>> {
        let rows = sqlx::query(
            r#"
            SELECT date, is_trading_day
            FROM trading_calendar
            WHERE date >= $1 AND date <= $2
            ORDER BY date
            "#,
        )
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(calendar_from_row).collect()
    }
}

#[async_trait]
impl FactorRepo for PostgresStore {
    async fn upsert_adjustment_factors(&self, factors: &[AdjustmentFactor]) -> Result<()> {
        let mut tx = self.pool.begin().await.map_err(other_error)?;
        for factor in factors {
            sqlx::query(
                r#"
                INSERT INTO adjustment_factors (symbol, ex_date, factor)
                VALUES ($1, $2, $3)
                ON CONFLICT (symbol, ex_date) DO UPDATE SET
                    factor = EXCLUDED.factor
                "#,
            )
            .bind(&factor.symbol)
            .bind(factor.ex_date)
            .bind(factor.factor)
            .execute(&mut *tx)
            .await
            .map_err(other_error)?;
        }
        tx.commit().await.map_err(other_error)
    }

    async fn query_adjustment_factors(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<AdjustmentFactor>> {
        let rows = sqlx::query(
            r#"
            SELECT symbol, ex_date, factor
            FROM adjustment_factors
            WHERE symbol = $1 AND ex_date >= $2 AND ex_date <= $3
            ORDER BY ex_date
            "#,
        )
        .bind(symbol)
        .bind(start)
        .bind(end)
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(adjustment_factor_from_row).collect()
    }
}

#[async_trait]
impl BacktestRunRepo for PostgresStore {
    async fn save_run(&self, run: BacktestRunRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO backtest_runs (
                id, strategy, symbols, config, status, metrics, created_at, finished_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (id) DO UPDATE SET
                strategy = EXCLUDED.strategy,
                symbols = EXCLUDED.symbols,
                config = EXCLUDED.config,
                status = EXCLUDED.status,
                metrics = EXCLUDED.metrics,
                finished_at = EXCLUDED.finished_at
            "#,
        )
        .bind(&run.id)
        .bind(&run.strategy)
        .bind(&run.symbols)
        .bind(Json(run.config))
        .bind(&run.status)
        .bind(run.metrics.map(Json))
        .bind(run.created_at)
        .bind(run.finished_at)
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn get_run(&self, id: &str) -> Result<Option<BacktestRunRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, strategy, symbols, config, status, metrics, created_at, finished_at
            FROM backtest_runs
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(other_error)?;

        row.map(backtest_run_from_row).transpose()
    }

    async fn list_runs(&self) -> Result<Vec<BacktestRunRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, strategy, symbols, config, status, metrics, created_at, finished_at
            FROM backtest_runs
            ORDER BY created_at DESC, id DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(backtest_run_from_row).collect()
    }
}

#[async_trait]
impl DecisionRepo for PostgresStore {
    async fn save_decision(&self, record: DecisionAuditRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO decisions (
                id, signal_id, symbol, exchange, action, side, target_quantity,
                rationale, risk_checks, analysis, pipeline_meta, source, ts
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
            ON CONFLICT (id) DO UPDATE SET
                signal_id = EXCLUDED.signal_id,
                symbol = EXCLUDED.symbol,
                exchange = EXCLUDED.exchange,
                action = EXCLUDED.action,
                side = EXCLUDED.side,
                target_quantity = EXCLUDED.target_quantity,
                rationale = EXCLUDED.rationale,
                risk_checks = EXCLUDED.risk_checks,
                analysis = EXCLUDED.analysis,
                pipeline_meta = EXCLUDED.pipeline_meta,
                source = EXCLUDED.source,
                ts = EXCLUDED.ts
            "#,
        )
        .bind(&record.decision.id)
        .bind(&record.decision.signal_id)
        .bind(&record.decision.symbol)
        .bind(exchange_to_str(record.decision.exchange))
        .bind(decision_action_to_str(record.decision.action))
        .bind(order_side_to_str(record.decision.side))
        .bind(record.decision.target_quantity)
        .bind(&record.decision.rationale)
        .bind(Json(
            serde_json::to_value(&record.decision.risk_checks).map_err(other_error)?,
        ))
        .bind(record.analysis.map(Json))
        .bind(record.pipeline_meta.map(Json))
        .bind(&record.source)
        .bind(record.decision.ts)
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn list_decisions(
        &self,
        symbol: Option<&str>,
        limit: i64,
    ) -> Result<Vec<DecisionAuditRecord>> {
        let limit = limit.clamp(1, 500);
        let rows = if let Some(symbol) = symbol {
            sqlx::query(
                r#"
                SELECT id, signal_id, symbol, exchange, action, side, target_quantity,
                       rationale, risk_checks, analysis, pipeline_meta, source, ts
                FROM decisions
                WHERE symbol = $1
                ORDER BY ts DESC, id DESC
                LIMIT $2
                "#,
            )
            .bind(symbol)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(other_error)?
        } else {
            sqlx::query(
                r#"
                SELECT id, signal_id, symbol, exchange, action, side, target_quantity,
                       rationale, risk_checks, analysis, pipeline_meta, source, ts
                FROM decisions
                ORDER BY ts DESC, id DESC
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(other_error)?
        };

        rows.into_iter().map(decision_audit_from_row).collect()
    }
}

#[async_trait]
impl OrderRepo for PostgresStore {
    async fn save_order(&self, order: &Order) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO orders (
                id, client_order_id, symbol, exchange, side, order_type, price,
                quantity, time_in_force, strategy_tag, source, status,
                filled_quantity, avg_fill_price, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'agent', $11, $12, $13, $14, now())
            ON CONFLICT (id) DO UPDATE SET
                status = EXCLUDED.status,
                filled_quantity = EXCLUDED.filled_quantity,
                avg_fill_price = EXCLUDED.avg_fill_price,
                updated_at = now()
            "#,
        )
        .bind(&order.id)
        .bind(&order.client_order_id)
        .bind(&order.symbol)
        .bind(exchange_to_str(order.exchange))
        .bind(order_side_to_str(order.side))
        .bind(order_type_to_str(order.order_type))
        .bind(order.price)
        .bind(order.quantity)
        .bind(time_in_force_to_str(order.time_in_force))
        .bind(strategy_style_to_str(order.strategy_tag))
        .bind(order_status_to_str(order.status))
        .bind(order.filled_quantity)
        .bind(order.avg_fill_price)
        .bind(order.created_at)
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn get_order(&self, id: &str) -> Result<Option<Order>> {
        let row = sqlx::query(
            r#"
            SELECT id, client_order_id, symbol, exchange, side, order_type, price,
                   quantity, time_in_force, strategy_tag, status, filled_quantity,
                   avg_fill_price, created_at
            FROM orders
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(other_error)?;

        row.map(order_from_row).transpose()
    }

    async fn list_open_orders(&self) -> Result<Vec<Order>> {
        let rows = sqlx::query(
            r#"
            SELECT id, client_order_id, symbol, exchange, side, order_type, price,
                   quantity, time_in_force, strategy_tag, status, filled_quantity,
                   avg_fill_price, created_at
            FROM orders
            WHERE status IN ('New', 'PartiallyFilled')
            ORDER BY created_at, id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(other_error)?;

        rows.into_iter().map(order_from_row).collect()
    }
}

#[async_trait]
impl FillRepo for PostgresStore {
    async fn save_fill(&self, fill: &Fill) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO fills (
                fill_id, order_id, symbol, exchange, side, price, quantity,
                commission, tax, transfer_fee, ts, trading_date
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (fill_id) DO NOTHING
            "#,
        )
        .bind(&fill.fill_id)
        .bind(&fill.order_id)
        .bind(&fill.symbol)
        .bind(exchange_to_str(fill.exchange))
        .bind(order_side_to_str(fill.side))
        .bind(fill.price)
        .bind(fill.quantity)
        .bind(fill.commission)
        .bind(fill.tax)
        .bind(fill.transfer_fee)
        .bind(fill.ts)
        .bind(fill.trading_date)
        .execute(&self.pool)
        .await
        .map_err(other_error)?;
        Ok(())
    }

    async fn list_fills(&self, order_id: Option<&str>, limit: i64) -> Result<Vec<Fill>> {
        let limit = limit.clamp(1, 1000);
        let rows = if let Some(order_id) = order_id {
            sqlx::query(
                r#"
                SELECT fill_id, order_id, symbol, exchange, side, price, quantity,
                       commission, tax, transfer_fee, ts, trading_date
                FROM fills
                WHERE order_id = $1
                ORDER BY ts DESC, fill_id DESC
                LIMIT $2
                "#,
            )
            .bind(order_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(other_error)?
        } else {
            sqlx::query(
                r#"
                SELECT fill_id, order_id, symbol, exchange, side, price, quantity,
                       commission, tax, transfer_fee, ts, trading_date
                FROM fills
                ORDER BY ts DESC, fill_id DESC
                LIMIT $1
                "#,
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(other_error)?
        };

        rows.into_iter().map(fill_from_row).collect()
    }
}

#[async_trait]
impl AccountStateRepo for PostgresStore {
    async fn save_account(&self, account: &Account, trading_date: NaiveDate) -> Result<()> {
        let unrealized_pnl = account
            .positions
            .values()
            .fold(Decimal::ZERO, |acc, position| acc + position.unrealized_pnl);
        let mut tx = self.pool.begin().await.map_err(other_error)?;
        sqlx::query(
            r#"
            INSERT INTO accounts (trading_date, cash, frozen_cash, total_value, unrealized_pnl)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(trading_date)
        .bind(account.cash)
        .bind(account.frozen_cash)
        .bind(account.total_value)
        .bind(unrealized_pnl)
        .execute(&mut *tx)
        .await
        .map_err(other_error)?;

        for position in account.positions.values() {
            sqlx::query(
                r#"
                INSERT INTO positions (symbol, trading_date, quantity, avg_cost, updated_at)
                VALUES ($1, $2, $3, $4, now())
                ON CONFLICT (symbol, trading_date) DO UPDATE SET
                    quantity = EXCLUDED.quantity,
                    avg_cost = EXCLUDED.avg_cost,
                    updated_at = now()
                "#,
            )
            .bind(&position.symbol)
            .bind(trading_date)
            .bind(position.total_quantity)
            .bind(position.avg_cost)
            .execute(&mut *tx)
            .await
            .map_err(other_error)?;
        }

        tx.commit().await.map_err(other_error)
    }
}

pub fn should_replace_latest_snapshot(
    existing_ts: Option<DateTime<Utc>>,
    incoming_ts: DateTime<Utc>,
) -> bool {
    existing_ts.map_or(true, |existing_ts| incoming_ts >= existing_ts)
}

async fn upsert_latest_snapshot(pool: &PgPool, snapshot: &Snapshot) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO latest_snapshots (
            symbol, ts, trading_date, last, open, high, low, pre_close,
            volume, amount, bid_price, bid_volume, ask_price, ask_volume
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        ON CONFLICT (symbol) DO UPDATE SET
            ts = EXCLUDED.ts,
            trading_date = EXCLUDED.trading_date,
            last = EXCLUDED.last,
            open = EXCLUDED.open,
            high = EXCLUDED.high,
            low = EXCLUDED.low,
            pre_close = EXCLUDED.pre_close,
            volume = EXCLUDED.volume,
            amount = EXCLUDED.amount,
            bid_price = EXCLUDED.bid_price,
            bid_volume = EXCLUDED.bid_volume,
            ask_price = EXCLUDED.ask_price,
            ask_volume = EXCLUDED.ask_volume
        WHERE latest_snapshots.ts <= EXCLUDED.ts
        "#,
    )
    .bind(&snapshot.symbol)
    .bind(snapshot.ts)
    .bind(snapshot.trading_date)
    .bind(snapshot.last)
    .bind(snapshot.open)
    .bind(snapshot.high)
    .bind(snapshot.low)
    .bind(snapshot.pre_close)
    .bind(snapshot.volume)
    .bind(snapshot.amount)
    .bind(snapshot.bid_price.to_vec())
    .bind(snapshot.bid_volume.to_vec())
    .bind(snapshot.ask_price.to_vec())
    .bind(snapshot.ask_volume.to_vec())
    .execute(pool)
    .await
    .map_err(other_error)?;
    Ok(())
}

fn instrument_from_row(row: sqlx::postgres::PgRow) -> Result<Instrument> {
    let list_date = row
        .try_get::<Option<NaiveDate>, _>("list_date")
        .map_err(other_error)?
        .ok_or_else(|| TgError::Validation("instrument.list_date is NULL".to_owned()))?;
    let exchange = row.try_get::<String, _>("exchange").map_err(other_error)?;
    let instrument_type = row
        .try_get::<String, _>("instrument_type")
        .map_err(other_error)?;
    let board = row.try_get::<String, _>("board").map_err(other_error)?;

    Ok(Instrument {
        symbol: row.try_get("symbol").map_err(other_error)?,
        exchange: exchange_from_str(&exchange)?,
        instrument_type: instrument_type_from_str(&instrument_type)?,
        name: row.try_get("name").map_err(other_error)?,
        list_date,
        delist_date: row.try_get("delist_date").map_err(other_error)?,
        is_st: row.try_get("is_st").map_err(other_error)?,
        board: board_from_str(&board)?,
    })
}

fn watchlist_from_row(row: sqlx::postgres::PgRow) -> Result<WatchlistEntry> {
    Ok(WatchlistEntry {
        symbol: row.try_get("symbol").map_err(other_error)?,
        strategy_tags: row.try_get("strategy_tags").map_err(other_error)?,
        added_at: row.try_get("added_at").map_err(other_error)?,
    })
}

fn calendar_from_row(row: sqlx::postgres::PgRow) -> Result<TradingCalendar> {
    Ok(TradingCalendar {
        date: row.try_get("date").map_err(other_error)?,
        is_trading_day: row.try_get("is_trading_day").map_err(other_error)?,
    })
}

fn adjustment_factor_from_row(row: sqlx::postgres::PgRow) -> Result<AdjustmentFactor> {
    Ok(AdjustmentFactor {
        symbol: row.try_get("symbol").map_err(other_error)?,
        ex_date: row.try_get("ex_date").map_err(other_error)?,
        factor: row.try_get("factor").map_err(other_error)?,
    })
}

fn backtest_run_from_row(row: sqlx::postgres::PgRow) -> Result<BacktestRunRecord> {
    let config = row
        .try_get::<Json<Value>, _>("config")
        .map_err(other_error)?
        .0;
    let metrics = row
        .try_get::<Option<Json<Value>>, _>("metrics")
        .map_err(other_error)?
        .map(|json| json.0);
    Ok(BacktestRunRecord {
        id: row.try_get("id").map_err(other_error)?,
        strategy: row.try_get("strategy").map_err(other_error)?,
        symbols: row.try_get("symbols").map_err(other_error)?,
        config,
        status: row.try_get("status").map_err(other_error)?,
        metrics,
        created_at: row.try_get("created_at").map_err(other_error)?,
        finished_at: row.try_get("finished_at").map_err(other_error)?,
    })
}

fn decision_audit_from_row(row: sqlx::postgres::PgRow) -> Result<DecisionAuditRecord> {
    let exchange = row.try_get::<String, _>("exchange").map_err(other_error)?;
    let action = row.try_get::<String, _>("action").map_err(other_error)?;
    let side = row.try_get::<String, _>("side").map_err(other_error)?;
    let risk_checks = row
        .try_get::<Json<Value>, _>("risk_checks")
        .map_err(other_error)?
        .0;
    let risk_checks =
        serde_json::from_value::<Vec<RiskCheckResult>>(risk_checks).map_err(other_error)?;
    let analysis = row
        .try_get::<Option<Json<Value>>, _>("analysis")
        .map_err(other_error)?
        .map(|json| json.0);
    let pipeline_meta = row
        .try_get::<Option<Json<Value>>, _>("pipeline_meta")
        .map_err(other_error)?
        .map(|json| json.0);

    Ok(DecisionAuditRecord {
        decision: Decision {
            id: row.try_get("id").map_err(other_error)?,
            signal_id: row.try_get("signal_id").map_err(other_error)?,
            symbol: row.try_get("symbol").map_err(other_error)?,
            exchange: exchange_from_str(&exchange)?,
            action: decision_action_from_str(&action)?,
            side: order_side_from_str(&side)?,
            target_quantity: row.try_get("target_quantity").map_err(other_error)?,
            rationale: row.try_get("rationale").map_err(other_error)?,
            risk_checks,
            ts: row.try_get("ts").map_err(other_error)?,
        },
        analysis,
        pipeline_meta,
        source: row.try_get("source").map_err(other_error)?,
    })
}

fn order_from_row(row: sqlx::postgres::PgRow) -> Result<Order> {
    let exchange = row.try_get::<String, _>("exchange").map_err(other_error)?;
    let side = row.try_get::<String, _>("side").map_err(other_error)?;
    let order_type = row
        .try_get::<String, _>("order_type")
        .map_err(other_error)?;
    let time_in_force = row
        .try_get::<String, _>("time_in_force")
        .map_err(other_error)?;
    let strategy_tag = row
        .try_get::<String, _>("strategy_tag")
        .map_err(other_error)?;
    let status = row.try_get::<String, _>("status").map_err(other_error)?;

    Ok(Order {
        id: row.try_get("id").map_err(other_error)?,
        client_order_id: row.try_get("client_order_id").map_err(other_error)?,
        symbol: row.try_get("symbol").map_err(other_error)?,
        exchange: exchange_from_str(&exchange)?,
        side: order_side_from_str(&side)?,
        order_type: order_type_from_str(&order_type)?,
        price: row.try_get("price").map_err(other_error)?,
        quantity: row.try_get("quantity").map_err(other_error)?,
        time_in_force: time_in_force_from_str(&time_in_force)?,
        strategy_tag: strategy_style_from_str(&strategy_tag)?,
        created_at: row.try_get("created_at").map_err(other_error)?,
        status: order_status_from_str(&status)?,
        filled_quantity: row.try_get("filled_quantity").map_err(other_error)?,
        avg_fill_price: row.try_get("avg_fill_price").map_err(other_error)?,
    })
}

fn fill_from_row(row: sqlx::postgres::PgRow) -> Result<Fill> {
    let exchange = row.try_get::<String, _>("exchange").map_err(other_error)?;
    let side = row.try_get::<String, _>("side").map_err(other_error)?;

    Ok(Fill {
        fill_id: row.try_get("fill_id").map_err(other_error)?,
        order_id: row.try_get("order_id").map_err(other_error)?,
        symbol: row.try_get("symbol").map_err(other_error)?,
        exchange: exchange_from_str(&exchange)?,
        side: order_side_from_str(&side)?,
        price: row.try_get("price").map_err(other_error)?,
        quantity: row.try_get("quantity").map_err(other_error)?,
        commission: row.try_get("commission").map_err(other_error)?,
        tax: row.try_get("tax").map_err(other_error)?,
        transfer_fee: row.try_get("transfer_fee").map_err(other_error)?,
        ts: row.try_get("ts").map_err(other_error)?,
        trading_date: row.try_get("trading_date").map_err(other_error)?,
    })
}

fn decision_action_to_str(action: DecisionAction) -> &'static str {
    match action {
        DecisionAction::Open => "Open",
        DecisionAction::Add => "Add",
        DecisionAction::Reduce => "Reduce",
        DecisionAction::Close => "Close",
        DecisionAction::Hold => "Hold",
    }
}

fn decision_action_from_str(value: &str) -> Result<DecisionAction> {
    match value {
        "Open" | "open" => Ok(DecisionAction::Open),
        "Add" | "add" => Ok(DecisionAction::Add),
        "Reduce" | "reduce" => Ok(DecisionAction::Reduce),
        "Close" | "close" => Ok(DecisionAction::Close),
        "Hold" | "hold" => Ok(DecisionAction::Hold),
        _ => Err(TgError::Validation(format!(
            "unknown decision action: {value}"
        ))),
    }
}

fn order_side_to_str(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "Buy",
        OrderSide::Sell => "Sell",
    }
}

fn order_side_from_str(value: &str) -> Result<OrderSide> {
    match value {
        "Buy" | "buy" => Ok(OrderSide::Buy),
        "Sell" | "sell" => Ok(OrderSide::Sell),
        _ => Err(TgError::Validation(format!("unknown order side: {value}"))),
    }
}

fn order_type_to_str(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Limit => "Limit",
        OrderType::Market => "Market",
    }
}

fn order_type_from_str(value: &str) -> Result<OrderType> {
    match value {
        "Limit" | "limit" => Ok(OrderType::Limit),
        "Market" | "market" => Ok(OrderType::Market),
        _ => Err(TgError::Validation(format!("unknown order type: {value}"))),
    }
}

fn time_in_force_to_str(time_in_force: TimeInForce) -> &'static str {
    match time_in_force {
        TimeInForce::Day => "Day",
        TimeInForce::Gtc => "Gtc",
    }
}

fn time_in_force_from_str(value: &str) -> Result<TimeInForce> {
    match value {
        "Day" | "day" => Ok(TimeInForce::Day),
        "Gtc" | "gtc" | "GTC" => Ok(TimeInForce::Gtc),
        _ => Err(TgError::Validation(format!(
            "unknown time in force: {value}"
        ))),
    }
}

fn strategy_style_to_str(strategy: StrategyStyle) -> &'static str {
    match strategy {
        StrategyStyle::Swing => "Swing",
        StrategyStyle::T0 => "T0",
        StrategyStyle::LimitUp => "LimitUp",
    }
}

fn strategy_style_from_str(value: &str) -> Result<StrategyStyle> {
    match value {
        "Swing" | "swing" => Ok(StrategyStyle::Swing),
        "T0" | "t0" => Ok(StrategyStyle::T0),
        "LimitUp" | "limitup" | "limit_up" => Ok(StrategyStyle::LimitUp),
        _ => Err(TgError::Validation(format!(
            "unknown strategy style: {value}"
        ))),
    }
}

fn order_status_to_str(status: OrderStatus) -> &'static str {
    match status {
        OrderStatus::New => "New",
        OrderStatus::PartiallyFilled => "PartiallyFilled",
        OrderStatus::Filled => "Filled",
        OrderStatus::Cancelled => "Cancelled",
        OrderStatus::Rejected => "Rejected",
    }
}

fn order_status_from_str(value: &str) -> Result<OrderStatus> {
    match value {
        "New" | "new" => Ok(OrderStatus::New),
        "PartiallyFilled" | "partially_filled" => Ok(OrderStatus::PartiallyFilled),
        "Filled" | "filled" => Ok(OrderStatus::Filled),
        "Cancelled" | "cancelled" | "canceled" => Ok(OrderStatus::Cancelled),
        "Rejected" | "rejected" => Ok(OrderStatus::Rejected),
        _ => Err(TgError::Validation(format!(
            "unknown order status: {value}"
        ))),
    }
}

fn snapshot_from_latest_row(row: sqlx::postgres::PgRow) -> Result<Snapshot> {
    let exchange = row
        .try_get::<Option<String>, _>("exchange")
        .map_err(other_error)?
        .ok_or_else(|| {
            TgError::Validation("latest snapshot has no matching instrument metadata".to_owned())
        })?;
    let bid_price = row
        .try_get::<Option<Vec<Decimal>>, _>("bid_price")
        .map_err(other_error)?
        .ok_or_else(|| TgError::Validation("latest snapshot bid_price is NULL".to_owned()))?;
    let bid_volume = row
        .try_get::<Option<Vec<i64>>, _>("bid_volume")
        .map_err(other_error)?
        .ok_or_else(|| TgError::Validation("latest snapshot bid_volume is NULL".to_owned()))?;
    let ask_price = row
        .try_get::<Option<Vec<Decimal>>, _>("ask_price")
        .map_err(other_error)?
        .ok_or_else(|| TgError::Validation("latest snapshot ask_price is NULL".to_owned()))?;
    let ask_volume = row
        .try_get::<Option<Vec<i64>>, _>("ask_volume")
        .map_err(other_error)?
        .ok_or_else(|| TgError::Validation("latest snapshot ask_volume is NULL".to_owned()))?;

    Ok(Snapshot {
        symbol: row.try_get("symbol").map_err(other_error)?,
        exchange: exchange_from_str(&exchange)?,
        ts: row.try_get("ts").map_err(other_error)?,
        trading_date: row.try_get("trading_date").map_err(other_error)?,
        last: row.try_get("last").map_err(other_error)?,
        open: row.try_get("open").map_err(other_error)?,
        high: row.try_get("high").map_err(other_error)?,
        low: row.try_get("low").map_err(other_error)?,
        pre_close: row.try_get("pre_close").map_err(other_error)?,
        volume: row.try_get("volume").map_err(other_error)?,
        amount: row.try_get("amount").map_err(other_error)?,
        bid_price: fixed_5(&bid_price, "bid_price")?,
        bid_volume: fixed_5(&bid_volume, "bid_volume")?,
        ask_price: fixed_5(&ask_price, "ask_price")?,
        ask_volume: fixed_5(&ask_volume, "ask_volume")?,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::should_replace_latest_snapshot;

    #[test]
    fn latest_snapshot_replace_logic_keeps_monotonic_timestamp() {
        let old = Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap();
        let same = Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap();
        let newer = Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 1).unwrap();
        let older = Utc.with_ymd_and_hms(2026, 6, 15, 1, 59, 59).unwrap();

        assert!(should_replace_latest_snapshot(None, old));
        assert!(should_replace_latest_snapshot(Some(old), same));
        assert!(should_replace_latest_snapshot(Some(old), newer));
        assert!(!should_replace_latest_snapshot(Some(old), older));
    }
}
