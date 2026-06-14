use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Utc};
use reqwest::Url;
use rust_decimal::Decimal;
use serde::Deserialize;
use tg_contracts::{
    AdjustmentFactor, Bar, BarPeriod, Board, Exchange, Instrument, InstrumentType, Result,
    Snapshot, TgError, TradingCalendar,
};

#[async_trait]
pub trait SidecarClient: Send + Sync {
    async fn health(&self) -> Result<()>;
    async fn get_instruments(&self, instrument_type: InstrumentType) -> Result<Vec<Instrument>>;
    async fn get_calendar(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<TradingCalendar>>;
    async fn get_bars(
        &self,
        symbol: &str,
        period: BarPeriod,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Bar>>;
    async fn get_snapshot(&self, symbols: &[String]) -> Result<Vec<Snapshot>>;
    async fn get_adjust_factors(&self, symbol: &str) -> Result<Vec<AdjustmentFactor>>;
}

#[derive(Debug, Clone)]
pub struct HttpSidecarClient {
    client: reqwest::Client,
    base_url: Url,
}

impl HttpSidecarClient {
    pub fn new(base_url: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::new(),
            base_url: Url::parse(base_url)
                .map_err(|err| TgError::Validation(format!("invalid sidecar base_url: {err}")))?,
        })
    }

    async fn get_json<T>(&self, path: &str, query: &[(&str, String)]) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = self
            .base_url
            .join(path)
            .map_err(|err| TgError::Validation(format!("invalid sidecar path {path}: {err}")))?;
        let response = self
            .client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|err| TgError::Upstream(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(TgError::Upstream(format!(
                "sidecar {path} returned {status}: {body}"
            )));
        }
        response
            .json::<T>()
            .await
            .map_err(|err| TgError::Upstream(format!("invalid sidecar JSON from {path}: {err}")))
    }
}

#[async_trait]
impl SidecarClient for HttpSidecarClient {
    async fn health(&self) -> Result<()> {
        let _: HealthWire = self.get_json("health", &[]).await?;
        Ok(())
    }

    async fn get_instruments(&self, instrument_type: InstrumentType) -> Result<Vec<Instrument>> {
        let kind = match instrument_type {
            InstrumentType::Stock => "stock",
            InstrumentType::Etf => "etf",
        };
        let rows: Vec<InstrumentWire> = self
            .get_json("instruments", &[("type", kind.to_owned())])
            .await?;
        rows.into_iter().map(Instrument::try_from).collect()
    }

    async fn get_calendar(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<TradingCalendar>> {
        let rows: Vec<TradingCalendarWire> = self
            .get_json(
                "calendar",
                &[("start", start.to_string()), ("end", end.to_string())],
            )
            .await?;
        rows.into_iter().map(TradingCalendar::try_from).collect()
    }

    async fn get_bars(
        &self,
        symbol: &str,
        period: BarPeriod,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Bar>> {
        let rows: Vec<BarWire> = self
            .get_json(
                "bars",
                &[
                    ("symbol", symbol.to_owned()),
                    ("period", period_to_query(period).to_owned()),
                    ("start", start.to_rfc3339()),
                    ("end", end.to_rfc3339()),
                ],
            )
            .await?;
        rows.into_iter().map(Bar::try_from).collect()
    }

    async fn get_snapshot(&self, symbols: &[String]) -> Result<Vec<Snapshot>> {
        let rows: Vec<SnapshotWire> = self
            .get_json("snapshot", &[("symbols", symbols.join(","))])
            .await?;
        rows.into_iter().map(Snapshot::try_from).collect()
    }

    async fn get_adjust_factors(&self, symbol: &str) -> Result<Vec<AdjustmentFactor>> {
        let rows: Vec<AdjustmentFactorWire> = self
            .get_json("adjust_factors", &[("symbol", symbol.to_owned())])
            .await?;
        rows.into_iter().map(AdjustmentFactor::try_from).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct MockSidecarClient;

impl MockSidecarClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SidecarClient for MockSidecarClient {
    async fn health(&self) -> Result<()> {
        Ok(())
    }

    async fn get_instruments(&self, instrument_type: InstrumentType) -> Result<Vec<Instrument>> {
        let instruments = fixture_instruments()
            .into_iter()
            .filter(|instrument| instrument.instrument_type == instrument_type)
            .collect();
        Ok(instruments)
    }

    async fn get_calendar(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<TradingCalendar>> {
        let mut days = Vec::new();
        let mut day = start;
        while day <= end {
            let weekday = day.weekday();
            days.push(TradingCalendar {
                date: day,
                is_trading_day: weekday.num_days_from_monday() < 5,
            });
            day = day
                .succ_opt()
                .ok_or_else(|| TgError::Validation("calendar date overflow".to_owned()))?;
        }
        Ok(days)
    }

    async fn get_bars(
        &self,
        symbol: &str,
        period: BarPeriod,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<Bar>> {
        let ts = Utc.with_ymd_and_hms(2026, 6, 15, 7, 0, 0).unwrap();
        if ts < start || ts > end {
            return Ok(Vec::new());
        }
        Ok(vec![Bar {
            symbol: symbol.to_owned(),
            exchange: exchange_for_symbol(symbol),
            period,
            ts,
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            open: dec(1000, 2),
            high: dec(1050, 2),
            low: dec(990, 2),
            close: dec(1020, 2),
            volume: 10_000,
            amount: dec(10_200_000, 2),
        }])
    }

    async fn get_snapshot(&self, symbols: &[String]) -> Result<Vec<Snapshot>> {
        let ts = Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap();
        Ok(symbols
            .iter()
            .map(|symbol| Snapshot {
                symbol: symbol.clone(),
                exchange: exchange_for_symbol(symbol),
                ts,
                trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
                last: dec(1020, 2),
                open: dec(1000, 2),
                high: dec(1050, 2),
                low: dec(990, 2),
                pre_close: dec(1000, 2),
                volume: 10_000,
                amount: dec(10_200_000, 2),
                bid_price: [dec(1019, 2); 5],
                bid_volume: [100; 5],
                ask_price: [dec(1021, 2); 5],
                ask_volume: [100; 5],
            })
            .collect())
    }

    async fn get_adjust_factors(&self, symbol: &str) -> Result<Vec<AdjustmentFactor>> {
        Ok(vec![AdjustmentFactor {
            symbol: symbol.to_owned(),
            ex_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            factor: dec(1000, 3),
        }])
    }
}

#[derive(Debug, Deserialize)]
struct HealthWire {
    #[allow(dead_code)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct InstrumentWire {
    symbol: String,
    exchange: String,
    instrument_type: String,
    name: String,
    list_date: String,
    #[serde(default)]
    delist_date: Option<String>,
    #[serde(default)]
    is_st: bool,
    board: String,
}

#[derive(Debug, Deserialize)]
struct BarWire {
    symbol: String,
    exchange: String,
    period: String,
    ts: String,
    trading_date: String,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: i64,
    amount: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotWire {
    symbol: String,
    exchange: String,
    ts: String,
    trading_date: String,
    last: String,
    open: String,
    high: String,
    low: String,
    pre_close: String,
    volume: i64,
    amount: String,
    bid_price: Vec<String>,
    bid_volume: Vec<i64>,
    ask_price: Vec<String>,
    ask_volume: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct AdjustmentFactorWire {
    symbol: String,
    ex_date: String,
    factor: String,
}

#[derive(Debug, Deserialize)]
struct TradingCalendarWire {
    date: String,
    is_trading_day: bool,
}

impl TryFrom<InstrumentWire> for Instrument {
    type Error = TgError;

    fn try_from(value: InstrumentWire) -> Result<Self> {
        Ok(Self {
            symbol: value.symbol,
            exchange: parse_exchange(&value.exchange)?,
            instrument_type: parse_instrument_type(&value.instrument_type)?,
            name: value.name,
            list_date: parse_date(&value.list_date)?,
            delist_date: value
                .delist_date
                .as_deref()
                .filter(|date| !date.is_empty())
                .map(parse_date)
                .transpose()?,
            is_st: value.is_st,
            board: parse_board(&value.board)?,
        })
    }
}

impl TryFrom<TradingCalendarWire> for TradingCalendar {
    type Error = TgError;

    fn try_from(value: TradingCalendarWire) -> Result<Self> {
        Ok(Self {
            date: parse_date(&value.date)?,
            is_trading_day: value.is_trading_day,
        })
    }
}

impl TryFrom<BarWire> for Bar {
    type Error = TgError;

    fn try_from(value: BarWire) -> Result<Self> {
        Ok(Self {
            symbol: value.symbol,
            exchange: parse_exchange(&value.exchange)?,
            period: parse_period(&value.period)?,
            ts: parse_ts(&value.ts)?,
            trading_date: parse_date(&value.trading_date)?,
            open: parse_decimal(&value.open)?,
            high: parse_decimal(&value.high)?,
            low: parse_decimal(&value.low)?,
            close: parse_decimal(&value.close)?,
            volume: value.volume,
            amount: parse_decimal(&value.amount)?,
        })
    }
}

impl TryFrom<SnapshotWire> for Snapshot {
    type Error = TgError;

    fn try_from(value: SnapshotWire) -> Result<Self> {
        Ok(Self {
            symbol: value.symbol,
            exchange: parse_exchange(&value.exchange)?,
            ts: parse_ts(&value.ts)?,
            trading_date: parse_date(&value.trading_date)?,
            last: parse_decimal(&value.last)?,
            open: parse_decimal(&value.open)?,
            high: parse_decimal(&value.high)?,
            low: parse_decimal(&value.low)?,
            pre_close: parse_decimal(&value.pre_close)?,
            volume: value.volume,
            amount: parse_decimal(&value.amount)?,
            bid_price: parse_decimal_array(value.bid_price, "bid_price")?,
            bid_volume: parse_i64_array(value.bid_volume, "bid_volume")?,
            ask_price: parse_decimal_array(value.ask_price, "ask_price")?,
            ask_volume: parse_i64_array(value.ask_volume, "ask_volume")?,
        })
    }
}

impl TryFrom<AdjustmentFactorWire> for AdjustmentFactor {
    type Error = TgError;

    fn try_from(value: AdjustmentFactorWire) -> Result<Self> {
        Ok(Self {
            symbol: value.symbol,
            ex_date: parse_date(&value.ex_date)?,
            factor: parse_decimal(&value.factor)?,
        })
    }
}

fn parse_decimal(value: &str) -> Result<Decimal> {
    Decimal::from_str(value)
        .map_err(|err| TgError::Validation(format!("invalid decimal {value}: {err}")))
}

fn parse_ts(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|ts| ts.with_timezone(&Utc))
        .map_err(|err| TgError::Validation(format!("invalid timestamp {value}: {err}")))
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|err| TgError::Validation(format!("invalid date {value}: {err}")))
}

fn parse_exchange(value: &str) -> Result<Exchange> {
    match value.to_ascii_lowercase().as_str() {
        "sh" | "exchange_sh" => Ok(Exchange::Sh),
        "sz" | "exchange_sz" => Ok(Exchange::Sz),
        "bj" | "exchange_bj" => Ok(Exchange::Bj),
        _ => Err(TgError::Validation(format!("invalid exchange {value}"))),
    }
}

fn parse_instrument_type(value: &str) -> Result<InstrumentType> {
    match value.to_ascii_lowercase().as_str() {
        "stock" | "instrument_type_stock" => Ok(InstrumentType::Stock),
        "etf" | "instrument_type_etf" => Ok(InstrumentType::Etf),
        _ => Err(TgError::Validation(format!(
            "invalid instrument type {value}"
        ))),
    }
}

fn parse_board(value: &str) -> Result<Board> {
    match value.to_ascii_lowercase().as_str() {
        "mainboard" | "main_board" | "board_main_board" => Ok(Board::MainBoard),
        "star" | "board_star" => Ok(Board::Star),
        "chinext" | "chi_next" | "board_chi_next" => Ok(Board::ChiNext),
        "bj" | "board_bj" => Ok(Board::Bj),
        _ => Err(TgError::Validation(format!("invalid board {value}"))),
    }
}

fn parse_period(value: &str) -> Result<BarPeriod> {
    match value.to_ascii_lowercase().as_str() {
        "daily" | "bar_period_daily" => Ok(BarPeriod::Daily),
        "min1" | "1m" | "bar_period_min1" => Ok(BarPeriod::Min1),
        "min5" | "5m" | "bar_period_min5" => Ok(BarPeriod::Min5),
        _ => Err(TgError::Validation(format!("invalid period {value}"))),
    }
}

fn period_to_query(period: BarPeriod) -> &'static str {
    match period {
        BarPeriod::Daily => "daily",
        BarPeriod::Min1 => "min1",
        BarPeriod::Min5 => "min5",
    }
}

fn parse_decimal_array(values: Vec<String>, field: &str) -> Result<[Decimal; 5]> {
    if values.len() != 5 {
        return Err(TgError::Validation(format!(
            "{field} expected 5 values, got {}",
            values.len()
        )));
    }
    let parsed = values
        .iter()
        .map(|value| parse_decimal(value))
        .collect::<Result<Vec<_>>>()?;
    parsed
        .try_into()
        .map_err(|_| TgError::Validation(format!("{field} expected exactly 5 decimal values")))
}

fn parse_i64_array(values: Vec<i64>, field: &str) -> Result<[i64; 5]> {
    values.try_into().map_err(|values: Vec<i64>| {
        TgError::Validation(format!("{field} expected 5 values, got {}", values.len()))
    })
}

fn fixture_instruments() -> Vec<Instrument> {
    vec![
        Instrument {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            instrument_type: InstrumentType::Stock,
            name: "Guizhou Maotai".to_owned(),
            list_date: NaiveDate::from_ymd_opt(2001, 8, 27).unwrap(),
            delist_date: None,
            is_st: false,
            board: Board::MainBoard,
        },
        Instrument {
            symbol: "159915".to_owned(),
            exchange: Exchange::Sz,
            instrument_type: InstrumentType::Etf,
            name: "ChiNext ETF".to_owned(),
            list_date: NaiveDate::from_ymd_opt(2011, 9, 20).unwrap(),
            delist_date: None,
            is_st: false,
            board: Board::ChiNext,
        },
    ]
}

fn exchange_for_symbol(symbol: &str) -> Exchange {
    if symbol.starts_with('6') {
        Exchange::Sh
    } else {
        Exchange::Sz
    }
}

fn dec(value: i64, scale: u32) -> Decimal {
    Decimal::new(value, scale)
}
