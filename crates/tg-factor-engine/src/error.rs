use tg_contracts::TgError;

#[derive(Debug, thiserror::Error)]
pub enum FactorError {
    #[error("unknown factor: {0}")]
    UnknownFactor(String),
    #[error("insufficient data for {factor}: need {needed}, got {actual}")]
    InsufficientData {
        factor: String,
        needed: usize,
        actual: usize,
    },
    #[error("invalid factor input: {0}")]
    InvalidInput(String),
    #[error("indicator upstream: {0}")]
    IndicatorUpstream(String),
    #[error("storage: {0}")]
    Storage(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, FactorError>;

impl From<FactorError> for TgError {
    fn from(value: FactorError) -> Self {
        match value {
            FactorError::UnknownFactor(message) => TgError::NotFound(message),
            FactorError::InsufficientData { .. } | FactorError::InvalidInput(_) => {
                TgError::Validation(value.to_string())
            }
            FactorError::IndicatorUpstream(message) => TgError::Upstream(message),
            FactorError::Storage(message) => TgError::Other(anyhow::anyhow!(message)),
            FactorError::Other(error) => TgError::Other(error),
        }
    }
}
