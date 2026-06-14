use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use tg_contracts::proto::tg::v1 as pb;
use tg_contracts::{Adjustment, BarPeriod, BarQuery, FactorEvaluation, FactorValue, TgError};
use tg_persistence::repo::BarRepo;
use tonic::{Request, Response, Status};

use crate::cross_section::standardize_cross_section;
use crate::error::{FactorError, Result};
use crate::evaluate::{evaluate, EvaluationInput, FactorReturn};
use crate::factor::FactorRegistry;
use crate::storage::FactorValueStore;

#[derive(Clone)]
pub struct FactorGrpcService<R>
where
    R: BarRepo + 'static,
{
    registry: FactorRegistry,
    bar_repo: Arc<R>,
    value_store: FactorValueStore,
}

impl<R> FactorGrpcService<R>
where
    R: BarRepo + 'static,
{
    pub fn new(registry: FactorRegistry, bar_repo: Arc<R>, value_store: FactorValueStore) -> Self {
        Self {
            registry,
            bar_repo,
            value_store,
        }
    }
}

#[async_trait]
impl<R> pb::factor_service_server::FactorService for FactorGrpcService<R>
where
    R: BarRepo + 'static,
{
    async fn compute_factor(
        &self,
        request: Request<pb::ComputeFactorRequest>,
    ) -> std::result::Result<Response<pb::ComputeFactorResponse>, Status> {
        let request = request.into_inner();
        let query = request
            .query
            .ok_or_else(|| Status::invalid_argument("query is required"))?;
        let factor = self
            .registry
            .get(&request.factor)
            .map_err(status_from_error)?;
        let symbols = if request.symbols.is_empty() {
            vec![query.symbol.clone()]
        } else {
            request.symbols
        };

        let mut values = Vec::new();
        for symbol in symbols {
            let mut symbol_query = bar_query_from_proto(&query).map_err(status_from_error)?;
            symbol_query.symbol = symbol.clone();
            let bars = self
                .bar_repo
                .query_bars(symbol_query)
                .await
                .map_err(status_from_tg_error)?;
            let series = factor
                .compute_timeseries(&bars)
                .await
                .map_err(status_from_error)?;
            values.extend(bars.iter().zip(series).map(|(bar, value)| FactorValue {
                symbol: symbol.clone(),
                factor: factor.meta().name.clone(),
                ts: bar.ts,
                trading_date: bar.trading_date,
                value,
                rank: None,
            }));
        }

        fill_cross_section_ranks(&mut values, factor.meta().direction);
        self.value_store
            .write_values(&values)
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(pb::ComputeFactorResponse {
            values: values.into_iter().map(factor_value_to_proto).collect(),
        }))
    }

    async fn evaluate_factor(
        &self,
        request: Request<pb::EvaluateFactorRequest>,
    ) -> std::result::Result<Response<pb::FactorEvaluation>, Status> {
        let request = request.into_inner();
        let query = request
            .query
            .ok_or_else(|| Status::invalid_argument("query is required"))?;
        self.registry
            .get(&request.factor)
            .map_err(status_from_error)?;
        let domain_query = bar_query_from_proto(&query).map_err(status_from_error)?;
        let symbols = if domain_query.symbol.is_empty() {
            Vec::new()
        } else {
            vec![domain_query.symbol.clone()]
        };
        let values = self
            .value_store
            .query_values(
                &request.factor,
                domain_query.range.start,
                domain_query.range.end,
                &symbols,
            )
            .await
            .map_err(status_from_error)?;
        let rows = evaluation_rows(&*self.bar_repo, &domain_query, &values)
            .await
            .map_err(status_from_error)?;
        let evaluation = evaluate(EvaluationInput {
            factor: request.factor,
            rows: rows.clone(),
            decay_rows: vec![rows],
            quantiles: 5,
        })
        .map_err(status_from_error)?;
        Ok(Response::new(factor_evaluation_to_proto(evaluation)))
    }

    async fn query_factor_values(
        &self,
        request: Request<pb::QueryFactorValuesRequest>,
    ) -> std::result::Result<Response<pb::ComputeFactorResponse>, Status> {
        let request = request.into_inner();
        let start = utc_from_ms(request.start_epoch_millis).map_err(status_from_error)?;
        let end = utc_from_ms(request.end_epoch_millis).map_err(status_from_error)?;
        let values = self
            .value_store
            .query_values(&request.factor, start, end, &request.symbols)
            .await
            .map_err(status_from_error)?;
        Ok(Response::new(pb::ComputeFactorResponse {
            values: values.into_iter().map(factor_value_to_proto).collect(),
        }))
    }
}

fn fill_cross_section_ranks(values: &mut [FactorValue], direction: crate::factor::FactorDirection) {
    let mut by_ts: BTreeMap<DateTime<Utc>, Vec<usize>> = BTreeMap::new();
    for (index, value) in values.iter().enumerate() {
        by_ts.entry(value.ts).or_default().push(index);
    }

    for indexes in by_ts.into_values() {
        let raw = indexes
            .iter()
            .map(|index| (values[*index].symbol.clone(), values[*index].value))
            .collect::<Vec<_>>();
        let ranked = standardize_cross_section(&raw, direction);
        for (index, ranked_value) in indexes.iter().zip(ranked) {
            values[*index].rank = ranked_value.rank;
        }
    }
}

async fn evaluation_rows<R>(
    bar_repo: &R,
    query: &BarQuery,
    values: &[FactorValue],
) -> Result<Vec<FactorReturn>>
where
    R: BarRepo + ?Sized,
{
    let mut out = Vec::new();
    for value in values {
        let mut symbol_query = query.clone();
        symbol_query.symbol = value.symbol.clone();
        symbol_query.range = value.ts..query.range.end;
        let bars = bar_repo
            .query_bars(symbol_query)
            .await
            .map_err(|error| FactorError::Storage(error.to_string()))?;
        if bars.len() < 2 {
            continue;
        }
        let current = bars[0].close.to_f64().unwrap_or(f64::NAN);
        let next = bars[1].close.to_f64().unwrap_or(f64::NAN);
        let forward_return = if current > 0.0 {
            next / current - 1.0
        } else {
            f64::NAN
        };
        out.push(FactorReturn {
            date: value.trading_date,
            symbol: value.symbol.clone(),
            factor_value: value.value,
            forward_return,
        });
    }
    Ok(out)
}

fn bar_query_from_proto(query: &pb::BarQuery) -> Result<BarQuery> {
    Ok(BarQuery {
        symbol: query.symbol.clone(),
        period: match pb::BarPeriod::from_i32(query.period) {
            Some(pb::BarPeriod::Daily) => BarPeriod::Daily,
            Some(pb::BarPeriod::Min1) => BarPeriod::Min1,
            Some(pb::BarPeriod::Min5) => BarPeriod::Min5,
            _ => {
                return Err(FactorError::InvalidInput(
                    "bar period is required".to_owned(),
                ))
            }
        },
        range: utc_from_ms(query.start_epoch_millis)?..utc_from_ms(query.end_epoch_millis)?,
        adjustment: match pb::Adjustment::from_i32(query.adjustment) {
            Some(pb::Adjustment::None) => Adjustment::None,
            Some(pb::Adjustment::PreAdjust) => Adjustment::PreAdjust,
            Some(pb::Adjustment::PostAdjust) => Adjustment::PostAdjust,
            _ => Adjustment::None,
        },
    })
}

pub fn factor_value_to_proto(value: FactorValue) -> pb::FactorValue {
    pb::FactorValue {
        symbol: value.symbol,
        factor: value.factor,
        ts_epoch_millis: value.ts.timestamp_millis(),
        trading_date: value.trading_date.to_string(),
        value: value.value,
        rank: value.rank.unwrap_or_default(),
        has_rank: value.rank.is_some(),
    }
}

pub fn factor_evaluation_to_proto(value: FactorEvaluation) -> pb::FactorEvaluation {
    pb::FactorEvaluation {
        factor: value.factor,
        ic_mean: value.ic_mean,
        ic_std: value.ic_std,
        ir: value.ir,
        decay: value.decay,
        quantile_returns: value.quantile_returns,
    }
}

fn utc_from_ms(value: i64) -> Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp_millis(value)
        .ok_or_else(|| FactorError::InvalidInput(format!("invalid timestamp millis: {value}")))
}

fn status_from_error(error: FactorError) -> Status {
    match error {
        FactorError::UnknownFactor(message) => Status::not_found(message),
        FactorError::InsufficientData { .. } | FactorError::InvalidInput(_) => {
            Status::invalid_argument(error.to_string())
        }
        FactorError::IndicatorUpstream(message) => Status::unavailable(message),
        FactorError::Storage(message) => Status::internal(message),
        FactorError::Other(error) => Status::internal(error.to_string()),
    }
}

fn status_from_tg_error(error: TgError) -> Status {
    match error {
        TgError::Validation(message) => Status::invalid_argument(message),
        TgError::NotFound(message) => Status::not_found(message),
        TgError::RateLimited => Status::resource_exhausted("rate limited"),
        TgError::Upstream(message) => Status::unavailable(message),
        TgError::InvalidOrder(message) | TgError::RiskRejected(message) => {
            Status::failed_precondition(message)
        }
        TgError::Other(error) => Status::internal(error.to_string()),
    }
}
