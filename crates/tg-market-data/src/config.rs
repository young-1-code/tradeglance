use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tg_contracts::{Result, TgError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub sidecar: SidecarConfig,
    pub grpc: BindConfig,
    pub health: BindConfig,
    pub database_url: String,
    pub data_root: PathBuf,
    pub watchlist_path: PathBuf,
    pub poll_interval_secs: u64,
    pub rate_limit: RateLimitConfig,
    pub retry: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarConfig {
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindConfig {
    pub bind_addr: SocketAddr,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub rate_per_sec: f64,
    pub burst: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchlistConfig {
    pub symbols: Vec<WatchlistSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchlistSymbol {
    pub symbol: String,
    #[serde(default)]
    pub strategy_tags: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sidecar: SidecarConfig {
                base_url: "http://127.0.0.1:8000/".to_owned(),
            },
            grpc: BindConfig {
                bind_addr: "127.0.0.1:50051".parse().expect("valid default grpc addr"),
            },
            health: BindConfig {
                bind_addr: "127.0.0.1:8080".parse().expect("valid default health addr"),
            },
            database_url: "postgres://postgres:postgres@localhost/tradeglance".to_owned(),
            data_root: PathBuf::from("data"),
            watchlist_path: PathBuf::from("crates/tg-market-data/config/watchlist.yaml"),
            poll_interval_secs: 5,
            rate_limit: RateLimitConfig {
                rate_per_sec: 2.0,
                burst: 4,
            },
            retry: RetryConfig {
                max_attempts: 3,
                base_delay_ms: 200,
                max_delay_ms: 5_000,
            },
        }
    }
}

impl AppConfig {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        match path {
            Some(path) => {
                let content = fs::read_to_string(path).map_err(|err| {
                    TgError::Validation(format!("failed to read config {}: {err}", path.display()))
                })?;
                serde_yaml::from_str(&content).map_err(|err| {
                    TgError::Validation(format!("failed to parse config {}: {err}", path.display()))
                })
            }
            None => Ok(Self::default()),
        }
    }
}

impl WatchlistConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).map_err(|err| {
            TgError::Validation(format!(
                "failed to read watchlist {}: {err}",
                path.display()
            ))
        })?;
        serde_yaml::from_str(&content).map_err(|err| {
            TgError::Validation(format!(
                "failed to parse watchlist {}: {err}",
                path.display()
            ))
        })
    }
}
