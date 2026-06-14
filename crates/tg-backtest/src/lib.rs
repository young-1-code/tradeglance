#![forbid(unsafe_code)]

pub mod grpc;
pub mod matcher;
pub mod perf;
pub mod replay;
pub mod runner;

pub use grpc::{BacktestJobExecutor, InProcessBacktestService, JobSnapshot};
pub use matcher::{BacktestLedger, CostBreakdown, HistoricalMatcher, MatcherConfig, PendingOrder};
pub use perf::{
    closed_trades_from_fills, compute_metrics, BacktestMetrics, BenchmarkMetrics, EquityPoint,
    Trade,
};
pub use replay::BacktestReplay;
pub use runner::{BacktestConfig, BacktestRunner, HistoricalClock, InstrumentedFeed};
