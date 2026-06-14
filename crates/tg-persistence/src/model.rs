use anyhow::anyhow;
use tg_contracts::{BarPeriod, Board, Exchange, InstrumentType, Result, TgError};

pub(crate) fn other_error(error: impl Into<anyhow::Error>) -> TgError {
    TgError::Other(error.into())
}

pub(crate) fn exchange_to_str(value: Exchange) -> &'static str {
    match value {
        Exchange::Sh => "sh",
        Exchange::Sz => "sz",
        Exchange::Bj => "bj",
    }
}

pub(crate) fn exchange_from_str(value: &str) -> Result<Exchange> {
    match value {
        "sh" | "SH" | "Sh" => Ok(Exchange::Sh),
        "sz" | "SZ" | "Sz" => Ok(Exchange::Sz),
        "bj" | "BJ" | "Bj" => Ok(Exchange::Bj),
        _ => Err(TgError::Validation(format!("unknown exchange: {value}"))),
    }
}

pub(crate) fn instrument_type_to_str(value: InstrumentType) -> &'static str {
    match value {
        InstrumentType::Stock => "stock",
        InstrumentType::Etf => "etf",
    }
}

pub(crate) fn instrument_type_from_str(value: &str) -> Result<InstrumentType> {
    match value {
        "stock" | "STOCK" | "Stock" => Ok(InstrumentType::Stock),
        "etf" | "ETF" | "Etf" => Ok(InstrumentType::Etf),
        _ => Err(TgError::Validation(format!(
            "unknown instrument type: {value}"
        ))),
    }
}

pub(crate) fn board_to_str(value: Board) -> &'static str {
    match value {
        Board::MainBoard => "main_board",
        Board::Star => "star",
        Board::ChiNext => "chi_next",
        Board::Bj => "bj",
    }
}

pub(crate) fn board_from_str(value: &str) -> Result<Board> {
    match value {
        "main_board" | "MainBoard" | "mainboard" => Ok(Board::MainBoard),
        "star" | "Star" => Ok(Board::Star),
        "chi_next" | "ChiNext" | "chinext" => Ok(Board::ChiNext),
        "bj" | "BJ" | "Bj" => Ok(Board::Bj),
        _ => Err(TgError::Validation(format!("unknown board: {value}"))),
    }
}

pub(crate) fn period_to_str(value: BarPeriod) -> &'static str {
    match value {
        BarPeriod::Daily => "daily",
        BarPeriod::Min1 => "minute1",
        BarPeriod::Min5 => "minute5",
    }
}

pub(crate) fn period_from_str(value: &str) -> Result<BarPeriod> {
    match value {
        "daily" | "Daily" => Ok(BarPeriod::Daily),
        "minute1" | "min1" | "Min1" => Ok(BarPeriod::Min1),
        "minute5" | "min5" | "Min5" => Ok(BarPeriod::Min5),
        _ => Err(TgError::Validation(format!("unknown bar period: {value}"))),
    }
}

pub(crate) fn fixed_5<T: Copy>(values: &[T], field: &str) -> Result<[T; 5]> {
    values
        .try_into()
        .map_err(|_| TgError::Validation(format!("{field} must contain exactly 5 values")))
}

pub(crate) fn invalid_data(message: impl Into<String>) -> TgError {
    TgError::Other(anyhow!(message.into()))
}
