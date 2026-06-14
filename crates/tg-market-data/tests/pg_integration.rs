#![cfg(feature = "pg_integration")]

use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDate;
use tg_market_data::{AsyncRateLimiter, MockSidecarClient, Repositories, SyncEngine, SyncOptions};
use tg_persistence::PostgresStore;

#[tokio::test]
async fn mock_sidecar_full_sync_writes_to_postgres_store() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("DATABASE_URL is not set; skipping pg_integration test");
        return;
    };
    let tempdir = tempfile::tempdir().expect("temp data root");
    let store = Arc::new(
        PostgresStore::connect(&database_url, tempdir.path())
            .await
            .expect("connect postgres"),
    );
    store.run_migrations().await.expect("run migrations");

    let engine = SyncEngine::new(
        Arc::new(MockSidecarClient::new()),
        Repositories::from_postgres_store(store.clone()),
        Arc::new(AsyncRateLimiter::new(1_000.0, 100)),
        SyncOptions {
            history_start: NaiveDate::from_ymd_opt(2026, 6, 14).unwrap(),
            history_end: NaiveDate::from_ymd_opt(2026, 6, 16).unwrap(),
            poll_interval: Duration::from_secs(1),
            ..SyncOptions::default()
        },
    );

    engine
        .full_sync(vec!["600519".to_owned()])
        .await
        .expect("full sync");
}
