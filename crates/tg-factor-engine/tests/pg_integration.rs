#![cfg(feature = "pg_integration")]
#![forbid(unsafe_code)]

use std::sync::Arc;

use chrono::{Duration, TimeZone, Utc};
use rust_decimal::Decimal;
use tg_contracts::{Adjustment, Bar, BarPeriod, BarQuery, Exchange};
use tg_factor_engine::factors::MomentumReturn;
use tg_factor_engine::storage::FactorValueStore;
use tg_factor_engine::{default_registry, Factor};
use tg_persistence::repo::{BarRepo, PostgresStore};

fn bar(day: u32, close: i64) -> Bar {
    let ts = Utc.with_ymd_and_hms(2026, 6, day, 7, 0, 0).unwrap();
    Bar {
        symbol: "600519".to_owned(),
        exchange: Exchange::Sh,
        period: BarPeriod::Daily,
        ts,
        trading_date: ts.date_naive(),
        open: Decimal::new(close, 0),
        high: Decimal::new(close, 0),
        low: Decimal::new(close, 0),
        close: Decimal::new(close, 0),
        volume: 10_000,
        amount: Decimal::new(close * 10_000, 0),
    }
}

#[tokio::test]
async fn pg_store_can_feed_factor_compute_and_parquet_values() {
    let Some(database_url) = std::env::var("DATABASE_URL").ok() else {
        eprintln!("DATABASE_URL is not set; skipping pg_integration smoke test");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let store = Arc::new(
        PostgresStore::connect(&database_url, temp.path())
            .await
            .expect("connect postgres"),
    );
    store
        .write_bars(&[bar(15, 10), bar(16, 11), bar(17, 12)])
        .await
        .expect("write bars");

    let bars = store
        .query_bars(BarQuery {
            symbol: "600519".to_owned(),
            period: BarPeriod::Daily,
            range: Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap()
                ..Utc.with_ymd_and_hms(2026, 6, 18, 0, 0, 0).unwrap() + Duration::days(1),
            adjustment: Adjustment::None,
        })
        .await
        .expect("query bars");
    let values = MomentumReturn::new(2)
        .compute_timeseries(&bars)
        .await
        .expect("compute factor");
    assert!((values[2] - 0.2).abs() < 1e-12);

    let registry = default_registry();
    assert!(registry.get("momentum_20d").is_ok());
    let factor_store = FactorValueStore::new(temp.path());
    assert!(factor_store.root().exists());
}
