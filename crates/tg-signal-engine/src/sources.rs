use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use tg_contracts::proto::tg::v1 as pb;
use tg_contracts::{Bar, BarPeriod, Exchange, FactorValue, Result, TgError};
use tonic::transport::Channel;

#[derive(Debug, Clone)]
pub struct IndicatorSeriesRequest {
    pub symbol: String,
    pub period: BarPeriod,
    pub indicator: String,
    pub params: HashMap<String, f64>,
    pub bars: Vec<Bar>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorSeries {
    pub indicator: String,
    pub ts: Vec<DateTime<Utc>>,
    pub series: HashMap<String, Vec<f64>>,
}

impl IndicatorSeries {
    pub fn latest(&self, key: &str) -> Option<f64> {
        self.series
            .get(key)
            .and_then(|values| values.last())
            .copied()
    }
}

#[async_trait]
pub trait IndicatorSource: Send + Sync {
    async fn fetch_indicator_series(
        &self,
        request: IndicatorSeriesRequest,
    ) -> Result<IndicatorSeries>;
}

#[derive(Debug, Clone)]
pub struct FactorValueRequest {
    pub factor: String,
    pub symbols: Vec<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[async_trait]
pub trait FactorSource: Send + Sync {
    async fn fetch_factor_values(&self, request: FactorValueRequest) -> Result<Vec<FactorValue>>;
}

#[derive(Debug, Default)]
pub struct InMemoryIndicatorSource {
    series: RwLock<HashMap<(String, String), IndicatorSeries>>,
}

impl InMemoryIndicatorSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(
        &self,
        symbol: impl Into<String>,
        indicator: impl Into<String>,
        series: IndicatorSeries,
    ) {
        self.series
            .write()
            .expect("indicator source lock should not be poisoned")
            .insert((symbol.into(), indicator.into()), series);
    }
}

#[async_trait]
impl IndicatorSource for InMemoryIndicatorSource {
    async fn fetch_indicator_series(
        &self,
        request: IndicatorSeriesRequest,
    ) -> Result<IndicatorSeries> {
        self.series
            .read()
            .expect("indicator source lock should not be poisoned")
            .get(&(request.symbol, request.indicator))
            .cloned()
            .ok_or_else(|| TgError::NotFound("indicator series not found".to_owned()))
    }
}

#[derive(Debug, Default)]
pub struct InMemoryFactorSource {
    values: RwLock<Vec<FactorValue>>,
}

impl InMemoryFactorSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, value: FactorValue) {
        self.values
            .write()
            .expect("factor source lock should not be poisoned")
            .push(value);
    }
}

#[async_trait]
impl FactorSource for InMemoryFactorSource {
    async fn fetch_factor_values(&self, request: FactorValueRequest) -> Result<Vec<FactorValue>> {
        let symbols = request
            .symbols
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        Ok(self
            .values
            .read()
            .expect("factor source lock should not be poisoned")
            .iter()
            .filter(|value| value.factor == request.factor)
            .filter(|value| symbols.is_empty() || symbols.contains(&value.symbol.as_str()))
            .filter(|value| value.ts >= request.start && value.ts <= request.end)
            .cloned()
            .collect())
    }
}

#[derive(Clone)]
pub struct GrpcIndicatorSource {
    client: Arc<tokio::sync::Mutex<pb::indicator_service_client::IndicatorServiceClient<Channel>>>,
}

impl GrpcIndicatorSource {
    pub async fn connect(endpoint: String) -> std::result::Result<Self, tonic::transport::Error> {
        let client = pb::indicator_service_client::IndicatorServiceClient::connect(endpoint).await?;
        Ok(Self {
            client: Arc::new(tokio::sync::Mutex::new(client)),
        })
    }
}

#[async_trait]
impl IndicatorSource for GrpcIndicatorSource {
    async fn fetch_indicator_series(
        &self,
        request: IndicatorSeriesRequest,
    ) -> Result<IndicatorSeries> {
        let pb_request = pb::IndicatorRequest {
            indicator: request.indicator,
            params: request.params,
            bars: request.bars.iter().map(bar_to_proto).collect(),
        };
        let response = self
            .client
            .lock()
            .await
            .compute(pb_request)
            .await
            .map_err(|error| TgError::Upstream(error.to_string()))?
            .into_inner();
        indicator_series_from_proto(response)
    }
}

#[derive(Clone)]
pub struct GrpcFactorSource {
    client: Arc<tokio::sync::Mutex<pb::factor_service_client::FactorServiceClient<Channel>>>,
}

impl GrpcFactorSource {
    pub async fn connect(endpoint: String) -> std::result::Result<Self, tonic::transport::Error> {
        let client = pb::factor_service_client::FactorServiceClient::connect(endpoint).await?;
        Ok(Self {
            client: Arc::new(tokio::sync::Mutex::new(client)),
        })
    }
}

#[async_trait]
impl FactorSource for GrpcFactorSource {
    async fn fetch_factor_values(&self, request: FactorValueRequest) -> Result<Vec<FactorValue>> {
        let pb_request = pb::QueryFactorValuesRequest {
            factor: request.factor,
            symbols: request.symbols,
            start_epoch_millis: request.start.timestamp_millis(),
            end_epoch_millis: request.end.timestamp_millis(),
        };
        let response = self
            .client
            .lock()
            .await
            .query_factor_values(pb_request)
            .await
            .map_err(|error| TgError::Upstream(error.to_string()))?
            .into_inner();
        response
            .values
            .into_iter()
            .map(factor_value_from_proto)
            .collect()
    }
}

fn indicator_series_from_proto(response: pb::IndicatorResult) -> Result<IndicatorSeries> {
    let ts = response
        .ts_epoch_millis
        .into_iter()
        .map(utc_from_ms)
        .collect::<Result<Vec<_>>>()?;
    let series = response
        .series
        .into_iter()
        .map(|(key, value)| (key, value.values))
        .collect();
    Ok(IndicatorSeries {
        indicator: response.indicator,
        ts,
        series,
    })
}

fn factor_value_from_proto(value: pb::FactorValue) -> Result<FactorValue> {
    Ok(FactorValue {
        symbol: value.symbol,
        factor: value.factor,
        ts: utc_from_ms(value.ts_epoch_millis)?,
        trading_date: NaiveDate::parse_from_str(&value.trading_date, "%Y-%m-%d")
            .map_err(|error| TgError::Upstream(error.to_string()))?,
        value: value.value,
        rank: value.has_rank.then_some(value.rank),
    })
}

fn bar_to_proto(bar: &Bar) -> pb::Bar {
    pb::Bar {
        symbol: bar.symbol.clone(),
        exchange: exchange_to_proto(bar.exchange) as i32,
        period: period_to_proto(bar.period) as i32,
        ts_epoch_millis: bar.ts.timestamp_millis(),
        trading_date: bar.trading_date.to_string(),
        open: bar.open.to_string(),
        high: bar.high.to_string(),
        low: bar.low.to_string(),
        close: bar.close.to_string(),
        volume: bar.volume,
        amount: bar.amount.to_string(),
    }
}

fn exchange_to_proto(exchange: Exchange) -> pb::Exchange {
    match exchange {
        Exchange::Sh => pb::Exchange::Sh,
        Exchange::Sz => pb::Exchange::Sz,
        Exchange::Bj => pb::Exchange::Bj,
    }
}

fn period_to_proto(period: BarPeriod) -> pb::BarPeriod {
    match period {
        BarPeriod::Daily => pb::BarPeriod::Daily,
        BarPeriod::Min1 => pb::BarPeriod::Min1,
        BarPeriod::Min5 => pb::BarPeriod::Min5,
    }
}

fn utc_from_ms(ms: i64) -> Result<DateTime<Utc>> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| TgError::Upstream(format!("invalid epoch millis: {ms}")))
}
