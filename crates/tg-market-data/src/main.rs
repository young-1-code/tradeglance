#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tg_contracts::proto::tg::v1::market_data_control_server::MarketDataControlServer;
use tg_market_data::{
    health_router, AppConfig, AsyncRateLimiter, HealthState, HttpSidecarClient, MarketDataService,
    Repositories, SyncEngine, SyncOptions, WatchlistConfig,
};
use tg_persistence::PostgresStore;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config_path = std::env::var_os("TG_MARKET_DATA_CONFIG")
        .map(PathBuf::from)
        .or_else(|| std::env::args_os().nth(1).map(PathBuf::from));
    let config = AppConfig::load(config_path.as_deref()).context("load market-data config")?;

    let store = Arc::new(
        PostgresStore::connect(&config.database_url, &config.data_root)
            .await
            .context("connect persistence store")?,
    );
    store.run_migrations().await.context("run migrations")?;

    let sidecar = Arc::new(HttpSidecarClient::new(&config.sidecar.base_url)?);
    let repos = Repositories::from_postgres_store(store.clone());
    let limiter = Arc::new(AsyncRateLimiter::new(
        config.rate_limit.rate_per_sec,
        config.rate_limit.burst,
    ));
    let engine = Arc::new(SyncEngine::new(
        sidecar.clone(),
        repos,
        limiter,
        SyncOptions {
            poll_interval: Duration::from_secs(config.poll_interval_secs),
            retry: config.retry,
            ..SyncOptions::default()
        },
    ));

    match WatchlistConfig::load(&config.watchlist_path) {
        Ok(watchlist) => engine.apply_watchlist_config(&watchlist.symbols).await?,
        Err(err) => warn!(error = %err, "watchlist config not applied"),
    }

    let cancellation = CancellationToken::new();
    let poller = {
        let engine = (*engine).clone();
        let cancellation = cancellation.clone();
        tokio::spawn(async move { engine.realtime_poll(cancellation).await })
    };

    let grpc_addr = config.grpc.bind_addr;
    let grpc = Server::builder()
        .add_service(MarketDataControlServer::new(MarketDataService::new(
            engine.clone(),
        )))
        .serve(grpc_addr);

    let health_addr = config.health.bind_addr;
    let health_listener = tokio::net::TcpListener::bind(health_addr)
        .await
        .with_context(|| format!("bind health server at {health_addr}"))?;
    let health = axum::serve(
        health_listener,
        health_router(HealthState {
            sidecar,
            pool: store.pool().clone(),
        }),
    );

    info!(%grpc_addr, %health_addr, "tg-market-data started");
    tokio::select! {
        result = grpc => result.context("grpc server failed")?,
        result = health => result.context("health server failed")?,
        result = poller => result.context("realtime poll task join failed")??,
    }
    cancellation.cancel();
    Ok(())
}
