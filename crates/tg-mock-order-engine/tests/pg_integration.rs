#![cfg(feature = "pg_integration")]

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use tg_contracts::{Exchange, OrderIntent, OrderSide, OrderType, StrategyStyle, TimeInForce};
use tg_engine::ExecutionHandler;
use tg_mock_order_engine::{MockExecutionConfig, MockExecutionHandler};
use tg_persistence::{AccountStateRepo, FillRepo, OrderRepo, PostgresStore};

fn snapshot() -> tg_contracts::Snapshot {
    tg_contracts::Snapshot {
        symbol: "600000".to_owned(),
        exchange: Exchange::Sh,
        ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
        trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        last: Decimal::new(10, 0),
        open: Decimal::new(10, 0),
        high: Decimal::new(10, 0),
        low: Decimal::new(10, 0),
        pre_close: Decimal::new(10, 0),
        volume: 10_000,
        amount: Decimal::new(100_000, 0),
        bid_price: [Decimal::new(999, 2); 5],
        bid_volume: [1_000; 5],
        ask_price: [Decimal::new(1001, 2); 5],
        ask_volume: [1_000; 5],
    }
}

#[tokio::test]
async fn full_paper_loop_persists_order_and_fill_when_database_url_is_set() -> anyhow::Result<()> {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("skipping pg_integration: DATABASE_URL is not set");
        return Ok(());
    };

    let store = Arc::new(PostgresStore::connect(&database_url, "/tmp/tradeglance-pg-it").await?);
    store.run_migrations().await?;
    let order_repo: Arc<dyn OrderRepo> = store.clone();
    let fill_repo: Arc<dyn FillRepo> = store.clone();
    let account_repo: Arc<dyn AccountStateRepo> = store.clone();
    let handler = MockExecutionHandler::new(MockExecutionConfig::default()).with_repos(
        order_repo,
        fill_repo,
        account_repo,
    );

    handler.on_snapshot(&snapshot()).await?;
    let order_id = handler
        .submit(OrderIntent {
            client_order_id: format!("pg-it-{}", Utc::now().timestamp_millis()),
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            price: Some(Decimal::new(1010, 2)),
            quantity: 100,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
        })
        .await?;
    let fills = handler.on_snapshot(&snapshot()).await?;

    assert!(!fills.is_empty());
    assert!(store.get_order(&order_id).await?.is_some());
    assert!(!store.list_fills(Some(&order_id), 10).await?.is_empty());
    Ok(())
}
