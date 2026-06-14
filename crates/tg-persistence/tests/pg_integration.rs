#![cfg(feature = "pg_integration")]

use chrono::{NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use tg_contracts::{
    Adjustment, Bar, BarPeriod, BarQuery, Board, Exchange, Instrument, InstrumentType,
    TradingCalendar,
};
use tg_persistence::{BarRepo, CalendarRepo, InstrumentRepo, PostgresStore};

fn dec(value: i64, scale: u32) -> Decimal {
    Decimal::new(value, scale)
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

#[tokio::test]
async fn postgres_repositories_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL not set; skipping pg_integration test");
        return Ok(());
    };

    let temp = tempfile::tempdir()?;
    let store = PostgresStore::connect(&database_url, temp.path()).await?;
    store.run_migrations().await?;

    sqlx::query("DELETE FROM watchlist WHERE symbol = $1")
        .bind("600519")
        .execute(store.pool())
        .await?;
    sqlx::query("DELETE FROM instruments WHERE symbol = $1")
        .bind("600519")
        .execute(store.pool())
        .await?;
    sqlx::query("DELETE FROM trading_calendar WHERE date = $1")
        .bind(date(2026, 6, 15))
        .execute(store.pool())
        .await?;

    let instrument = Instrument {
        symbol: "600519".to_owned(),
        exchange: Exchange::Sh,
        instrument_type: InstrumentType::Stock,
        name: "Kweichow Moutai".to_owned(),
        list_date: date(2001, 8, 27),
        delist_date: None,
        is_st: false,
        board: Board::MainBoard,
    };
    store.upsert_instrument(&instrument).await?;
    assert_eq!(store.get_instrument("600519").await?, Some(instrument));

    store
        .upsert_calendar(&[TradingCalendar {
            date: date(2026, 6, 15),
            is_trading_day: true,
        }])
        .await?;
    assert!(store.is_trading_day(date(2026, 6, 15)).await?);

    let bar = Bar {
        symbol: "600519".to_owned(),
        exchange: Exchange::Sh,
        period: BarPeriod::Daily,
        ts: Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap(),
        trading_date: date(2026, 6, 15),
        open: dec(150_0000, 4),
        high: dec(151_0000, 4),
        low: dec(149_0000, 4),
        close: dec(150_5000, 4),
        volume: 10_000,
        amount: dec(1_505_000_0000, 4),
    };
    store.write_bars(std::slice::from_ref(&bar)).await?;
    let bars = store
        .query_bars(BarQuery {
            symbol: "600519".to_owned(),
            period: BarPeriod::Daily,
            range: Utc.with_ymd_and_hms(2026, 6, 15, 0, 0, 0).unwrap()
                ..Utc.with_ymd_and_hms(2026, 6, 16, 0, 0, 0).unwrap(),
            adjustment: Adjustment::None,
        })
        .await?;
    assert_eq!(bars, vec![bar]);

    Ok(())
}
