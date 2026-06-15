#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use tg_contracts::proto::tg::v1::order_service_server::OrderServiceServer;
use tg_contracts::{Bar, Fill, Snapshot, StrategyStyle};
use tg_engine::{Clock, Engine, InMemoryPortfolio, Strategy, StrategyContext};
use tg_mock_order_engine::{
    MockExecutionConfig, MockExecutionHandler, OrderGrpcService, RealtimeDataFeed,
};
use tg_persistence::{AccountStateRepo, FillRepo, OrderRepo, PostgresStore, SnapshotRepo};
use tonic::transport::Server;

#[derive(Debug, Default)]
struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn trading_date(&self, ts: DateTime<Utc>) -> NaiveDate {
        ts.date_naive()
    }
}

struct ExecutionPump {
    handler: Arc<MockExecutionHandler>,
}

#[async_trait]
impl Strategy for ExecutionPump {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> tg_contracts::Result<()> {
        Ok(())
    }

    async fn on_bar(
        &mut self,
        _bar: &Bar,
        _ctx: &mut StrategyContext<'_>,
    ) -> tg_contracts::Result<()> {
        Ok(())
    }

    async fn on_snapshot(
        &mut self,
        snap: &Snapshot,
        _ctx: &mut StrategyContext<'_>,
    ) -> tg_contracts::Result<()> {
        self.handler.on_snapshot(snap).await.map(|_| ())
    }

    async fn on_timer(
        &mut self,
        _at: DateTime<Utc>,
        _ctx: &mut StrategyContext<'_>,
    ) -> tg_contracts::Result<()> {
        Ok(())
    }

    async fn on_fill(
        &mut self,
        _fill: &Fill,
        _ctx: &mut StrategyContext<'_>,
    ) -> tg_contracts::Result<()> {
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut StrategyContext<'_>) -> tg_contracts::Result<()> {
        Ok(())
    }

    fn style(&self) -> StrategyStyle {
        StrategyStyle::Swing
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let addr: SocketAddr = std::env::var("TG_MOCK_ORDER_ENGINE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50055".to_owned())
        .parse()?;
    let mut handler = MockExecutionHandler::new(MockExecutionConfig::default());

    let maybe_store = match std::env::var("TG_DATABASE_URL") {
        Ok(database_url) => {
            let data_root = std::env::var("TG_DATA_ROOT").unwrap_or_else(|_| "data".to_owned());
            let store = Arc::new(PostgresStore::connect(&database_url, data_root).await?);
            store.run_migrations().await?;
            let order_repo: Arc<dyn OrderRepo> = store.clone();
            let fill_repo: Arc<dyn FillRepo> = store.clone();
            let account_repo: Arc<dyn AccountStateRepo> = store.clone();
            handler = handler.with_repos(order_repo, fill_repo, account_repo);
            Some(store)
        }
        Err(_) => None,
    };

    let handler = Arc::new(handler);
    if let Some(store) = maybe_store {
        if let Ok(raw_symbols) = std::env::var("TG_WATCHLIST_SYMBOLS") {
            let symbols: Vec<String> = raw_symbols
                .split(',')
                .map(str::trim)
                .filter(|symbol| !symbol.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if !symbols.is_empty() {
                let snapshot_repo: Arc<dyn SnapshotRepo> = store;
                let feed = RealtimeDataFeed::new(
                    snapshot_repo,
                    symbols,
                    std::time::Duration::from_secs(1),
                );
                let mut engine = Engine::builder()
                    .with_feed(feed)
                    .with_executor(handler.clone())
                    .with_clock(Arc::new(WallClock))
                    .with_portfolio(Arc::new(InMemoryPortfolio::default()))
                    .with_strategy(ExecutionPump {
                        handler: handler.clone(),
                    })
                    .build()?;
                tokio::spawn(async move {
                    if let Err(error) = engine.run().await {
                        tracing::error!(%error, "paper trading engine stopped with error");
                    }
                });
            }
        }
    }

    let grpc = OrderGrpcService::new(handler);

    tracing::info!(%addr, "starting tg-mock-order-engine gRPC service");
    Server::builder()
        .add_service(OrderServiceServer::new(grpc))
        .serve(addr)
        .await?;
    Ok(())
}
