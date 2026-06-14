use std::sync::Arc;

use async_trait::async_trait;
use tg_contracts::proto::tg::v1::market_data_control_server::MarketDataControl;
use tg_contracts::proto::tg::v1::{
    Empty, FullSyncRequest, SyncJob, SyncStatus, SyncStatusReport, Watchlist, WatchlistDelta,
};
use tg_contracts::{Result, TgError};
use tonic::{Request, Response, Status};

use crate::sync::SyncEngine;

#[async_trait]
pub trait ControlPlane: Send + Sync + 'static {
    async fn trigger_full_sync(&self, symbols: Vec<String>) -> Result<SyncJob>;
    async fn trigger_incremental_sync(&self) -> Result<SyncJob>;
    async fn sync_status(&self) -> Result<Vec<SyncStatus>>;
    async fn update_watchlist(&self, add: Vec<String>, remove: Vec<String>) -> Result<Vec<String>>;
    async fn watchlist(&self) -> Result<Vec<String>>;
}

#[async_trait]
impl ControlPlane for SyncEngine {
    async fn trigger_full_sync(&self, symbols: Vec<String>) -> Result<SyncJob> {
        Ok(self.trigger_full_sync(symbols).await)
    }

    async fn trigger_incremental_sync(&self) -> Result<SyncJob> {
        Ok(self.trigger_incremental_sync().await)
    }

    async fn sync_status(&self) -> Result<Vec<SyncStatus>> {
        self.sync_status_report().await
    }

    async fn update_watchlist(&self, add: Vec<String>, remove: Vec<String>) -> Result<Vec<String>> {
        self.update_watchlist(add, remove).await
    }

    async fn watchlist(&self) -> Result<Vec<String>> {
        self.watchlist_symbols().await
    }
}

#[derive(Clone)]
pub struct MarketDataService<C = SyncEngine> {
    control: Arc<C>,
}

impl<C> MarketDataService<C> {
    pub fn new(control: Arc<C>) -> Self {
        Self { control }
    }
}

#[tonic::async_trait]
impl<C> MarketDataControl for MarketDataService<C>
where
    C: ControlPlane,
{
    async fn trigger_full_sync(
        &self,
        request: Request<FullSyncRequest>,
    ) -> std::result::Result<Response<SyncJob>, Status> {
        let job = self
            .control
            .trigger_full_sync(request.into_inner().symbols)
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(job))
    }

    async fn trigger_incremental_sync(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<SyncJob>, Status> {
        let job = self
            .control
            .trigger_incremental_sync()
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(job))
    }

    async fn get_sync_status(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<SyncStatusReport>, Status> {
        let statuses = self
            .control
            .sync_status()
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(SyncStatusReport { statuses }))
    }

    async fn update_watchlist(
        &self,
        request: Request<WatchlistDelta>,
    ) -> std::result::Result<Response<Watchlist>, Status> {
        let delta = request.into_inner();
        let symbols = self
            .control
            .update_watchlist(delta.add_symbols, delta.remove_symbols)
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(Watchlist { symbols }))
    }

    async fn get_watchlist(
        &self,
        _request: Request<Empty>,
    ) -> std::result::Result<Response<Watchlist>, Status> {
        let symbols = self.control.watchlist().await.map_err(status_from_error)?;
        Ok(Response::new(Watchlist { symbols }))
    }
}

fn status_from_error(error: TgError) -> Status {
    match error {
        TgError::Validation(message) | TgError::InvalidOrder(message) => {
            Status::invalid_argument(message)
        }
        TgError::NotFound(message) => Status::not_found(message),
        TgError::RateLimited => Status::resource_exhausted("rate limited"),
        TgError::RiskRejected(message) => Status::failed_precondition(message),
        TgError::Upstream(message) => Status::unavailable(message),
        TgError::Other(error) => Status::internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use tg_contracts::proto::tg::v1::BarPeriod as ProtoBarPeriod;

    use super::*;

    #[derive(Default)]
    struct MockControl {
        watchlist: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ControlPlane for MockControl {
        async fn trigger_full_sync(&self, symbols: Vec<String>) -> Result<SyncJob> {
            Ok(SyncJob {
                id: format!("full-{}", symbols.len()),
                status: "running".to_owned(),
                created_at_epoch_millis: 1,
            })
        }

        async fn trigger_incremental_sync(&self) -> Result<SyncJob> {
            Ok(SyncJob {
                id: "incremental".to_owned(),
                status: "running".to_owned(),
                created_at_epoch_millis: 2,
            })
        }

        async fn sync_status(&self) -> Result<Vec<SyncStatus>> {
            Ok(vec![SyncStatus {
                symbol: "600519".to_owned(),
                period: ProtoBarPeriod::Daily as i32,
                status: "idle".to_owned(),
                last_fetched_ts_epoch_millis: 1,
                last_sync_at_epoch_millis: 2,
                last_error: String::new(),
            }])
        }

        async fn update_watchlist(
            &self,
            add: Vec<String>,
            remove: Vec<String>,
        ) -> Result<Vec<String>> {
            let mut watchlist = self.watchlist.lock().unwrap();
            watchlist.retain(|symbol| !remove.iter().any(|remove| remove == symbol));
            for symbol in add {
                if !watchlist.contains(&symbol) {
                    watchlist.push(symbol);
                }
            }
            Ok(watchlist.clone())
        }

        async fn watchlist(&self) -> Result<Vec<String>> {
            Ok(self.watchlist.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn grpc_methods_return_expected_shapes() {
        let service = MarketDataService::new(Arc::new(MockControl::default()));

        let full = service
            .trigger_full_sync(Request::new(FullSyncRequest {
                symbols: vec!["600519".to_owned()],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(full.id, "full-1");

        let watchlist = service
            .update_watchlist(Request::new(WatchlistDelta {
                add_symbols: vec!["600519".to_owned()],
                remove_symbols: vec![],
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(watchlist.symbols, vec!["600519"]);

        let status = service
            .get_sync_status(Request::new(Empty {}))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(status.statuses.len(), 1);
    }
}
