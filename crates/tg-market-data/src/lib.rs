#![forbid(unsafe_code)]

pub mod cleaning;
pub mod config;
pub mod grpc;
pub mod health;
pub mod rate_limit;
pub mod sidecar;
pub mod sync;

pub use cleaning::{
    flag_adjustment_gaps, is_price_within_limit, is_suspended_bar, is_suspended_snapshot,
    missing_trading_days, AdjustmentGap,
};
pub use config::{AppConfig, RateLimitConfig, WatchlistConfig, WatchlistSymbol};
pub use grpc::MarketDataService;
pub use health::{health_router, HealthState};
pub use rate_limit::{AsyncRateLimiter, TokenBucket};
pub use sidecar::{HttpSidecarClient, MockSidecarClient, SidecarClient};
pub use sync::{FetchState, FetchStateRepo, Repositories, SyncEngine, SyncOptions};
