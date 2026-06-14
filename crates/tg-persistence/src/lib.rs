#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]
//!
//! ## Concurrency model
//!
//! PostgreSQL access relies on MVCC for concurrent readers and writers.
//! Parquet history follows the Phase 0 single-writer model: `tg-market-data`
//! is the only writer, writes complete partitions through a same-directory temp
//! file and atomic rename, and other modules read the files read-only.

pub mod adjust;
mod model;
pub mod parquet_io;
pub mod repo;

pub use adjust::adjust_bars;
pub use parquet_io::{
    bar_partition_path, snapshot_partition_path, BarPartition, ParquetStore, SnapshotPartition,
};
pub use repo::{
    should_replace_latest_snapshot, BarRepo, CalendarRepo, FactorRepo, InstrumentRepo,
    PostgresStore, SnapshotRepo, WatchlistEntry,
};
