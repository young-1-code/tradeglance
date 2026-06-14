#![cfg(feature = "pg_integration")]

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::json;
use std::ops::Range;
use std::sync::Mutex;
use tg_backtest::{BacktestConfig, BacktestRunner, MatcherConfig};
use tg_contracts::{
    Adjustment, Bar, BarPeriod, Fill, OrderIntent, OrderSide, OrderType, Result, Snapshot,
    StrategyStyle, TimeInForce,
};
use tg_engine::{Strategy, StrategyContext};
use tg_persistence::PostgresStore;

struct BuyFirstBarStrategy {
    bought: Mutex<bool>,
}

#[async_trait]
impl Strategy for BuyFirstBarStrategy {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()> {
        let mut bought = self
            .bought
            .lock()
            .expect("strategy lock should not be poisoned");
        if !*bought {
            ctx.broker
                .submit(OrderIntent {
                    client_order_id: "pg-integration-buy".to_owned(),
                    symbol: bar.symbol.clone(),
                    exchange: bar.exchange,
                    side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    price: Some(bar.close),
                    quantity: 100,
                    time_in_force: TimeInForce::Day,
                    strategy_tag: StrategyStyle::Swing,
                })
                .await?;
            *bought = true;
        }
        Ok(())
    }

    async fn on_snapshot(
        &mut self,
        _snap: &Snapshot,
        _ctx: &mut StrategyContext<'_>,
    ) -> Result<()> {
        Ok(())
    }

    async fn on_timer(&mut self, _at: DateTime<Utc>, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_fill(&mut self, _fill: &Fill, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    fn style(&self) -> StrategyStyle {
        StrategyStyle::Swing
    }
}

#[tokio::test]
async fn full_run_against_postgres_parquet_store_when_configured() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping pg integration test");
        return;
    };
    let symbol = std::env::var("TG_BACKTEST_TEST_SYMBOL").unwrap_or_else(|_| "600001".to_owned());
    let data_root =
        std::env::var("TG_DATA_ROOT").unwrap_or_else(|_| "/tmp/tradeglance-data".to_owned());
    let start = std::env::var("TG_BACKTEST_START")
        .ok()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(|| "2026-01-01T00:00:00Z".parse().unwrap());
    let end = std::env::var("TG_BACKTEST_END")
        .ok()
        .and_then(|value| value.parse::<DateTime<Utc>>().ok())
        .unwrap_or_else(|| "2027-01-01T00:00:00Z".parse().unwrap());

    let store = std::sync::Arc::new(
        PostgresStore::connect(&database_url, data_root)
            .await
            .expect("connect postgres store"),
    );
    let runner = BacktestRunner::new(store);
    let output = runner
        .run(
            BacktestConfig {
                id: "pg-integration".to_owned(),
                strategy: "buy_first_bar".to_owned(),
                symbols: vec![symbol],
                period: BarPeriod::Daily,
                range: Range { start, end },
                adjustment: Adjustment::None,
                matcher: MatcherConfig {
                    initial_cash: Decimal::new(1_000_000, 0),
                    ..MatcherConfig::default()
                },
                config_json: json!({"test": "pg_integration"}),
            },
            vec![Box::new(BuyFirstBarStrategy {
                bought: Mutex::new(false),
            })],
        )
        .await
        .expect("run backtest");

    assert!(output.summary.bars_processed > 0);
    assert!(output.metrics.total_return.is_finite());
    assert!(output.metrics.sharpe.is_finite());
}
