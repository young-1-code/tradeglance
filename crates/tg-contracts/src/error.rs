#[derive(Debug, thiserror::Error)]
pub enum TgError {
    #[error("data validation: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("rate limited")]
    RateLimited,
    #[error("upstream: {0}")]
    Upstream(String),
    #[error("invalid order: {0}")]
    InvalidOrder(String),
    #[error("risk rejected: {0}")]
    RiskRejected(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, TgError>;
