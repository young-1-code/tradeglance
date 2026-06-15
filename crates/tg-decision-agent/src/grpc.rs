use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use tg_contracts::proto::tg::v1 as pb;
use tg_contracts::{
    Decision, DecisionAction, Exchange, OrderSide, RiskCheckResult, Signal, SignalDirection,
    StrategyStyle, TgError,
};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::agents::DecisionEngine;
use crate::context::{ContextPayload, DecisionContext};

pub struct DecisionGrpcService {
    engine: Arc<dyn DecisionEngine>,
    tx: broadcast::Sender<Decision>,
}

impl DecisionGrpcService {
    pub fn new(engine: Arc<dyn DecisionEngine>) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self { engine, tx }
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
#[tonic::async_trait]
impl pb::decision_service_server::DecisionService for DecisionGrpcService {
    type SubscribeDecisionsStream =
        Pin<Box<dyn Stream<Item = std::result::Result<pb::Decision, Status>> + Send + 'static>>;

    async fn decide(
        &self,
        request: Request<pb::DecisionRequest>,
    ) -> std::result::Result<Response<pb::Decision>, Status> {
        let request = request.into_inner();
        let signal = request
            .signal
            .ok_or_else(|| Status::invalid_argument("signal is required"))
            .and_then(signal_from_proto)?;
        let payload = ContextPayload::from_json(&request.context_json)
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let context = DecisionContext::new(signal, payload);
        let decision = self
            .engine
            .decide(context)
            .await
            .map_err(status_from_error)?;
        let _ = self.tx.send(decision.clone());
        Ok(Response::new(decision_to_proto(decision)))
    }

    async fn subscribe_decisions(
        &self,
        request: Request<pb::SubscribeDecisionsRequest>,
    ) -> std::result::Result<Response<Self::SubscribeDecisionsStream>, Status> {
        let request = request.into_inner();
        let symbols = request.symbols;
        let mut rx = self.tx.subscribe();
        let (tx, out_rx) = mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(decision) => {
                        if !symbols.is_empty() && !symbols.contains(&decision.symbol) {
                            continue;
                        }
                        if tx.send(Ok(decision_to_proto(decision))).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }
}

pub fn decision_to_proto(decision: Decision) -> pb::Decision {
    pb::Decision {
        id: decision.id,
        signal_id: decision.signal_id.clone().unwrap_or_default(),
        has_signal_id: decision.signal_id.is_some(),
        symbol: decision.symbol,
        exchange: exchange_to_proto(decision.exchange) as i32,
        action: action_to_proto(decision.action) as i32,
        side: side_to_proto(decision.side) as i32,
        target_quantity: decision.target_quantity,
        rationale: decision.rationale,
        risk_checks: decision
            .risk_checks
            .into_iter()
            .map(risk_to_proto)
            .collect(),
        ts_epoch_millis: decision.ts.timestamp_millis(),
    }
}

fn risk_to_proto(risk: RiskCheckResult) -> pb::RiskCheckResult {
    pb::RiskCheckResult {
        rule: risk.rule,
        passed: risk.passed,
        detail: risk.detail,
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
fn signal_from_proto(signal: pb::Signal) -> std::result::Result<Signal, Status> {
    Ok(Signal {
        id: signal.id,
        symbol: signal.symbol,
        exchange: exchange_from_i32(signal.exchange)?,
        direction: direction_from_i32(signal.direction)?,
        strength: signal.strength,
        confidence: signal.confidence,
        style: style_from_i32(signal.style)?,
        reason: signal.reason,
        suggested_quantity: signal
            .has_suggested_quantity
            .then_some(signal.suggested_quantity),
        ts: utc_from_ms(signal.ts_epoch_millis)?,
        trading_date: NaiveDate::parse_from_str(&signal.trading_date, "%Y-%m-%d")
            .map_err(|error| Status::invalid_argument(error.to_string()))?,
    })
}

fn exchange_to_proto(exchange: Exchange) -> pb::Exchange {
    match exchange {
        Exchange::Sh => pb::Exchange::Sh,
        Exchange::Sz => pb::Exchange::Sz,
        Exchange::Bj => pb::Exchange::Bj,
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
fn exchange_from_i32(value: i32) -> std::result::Result<Exchange, Status> {
    match pb::Exchange::from_i32(value) {
        Some(pb::Exchange::Sh) => Ok(Exchange::Sh),
        Some(pb::Exchange::Sz) => Ok(Exchange::Sz),
        Some(pb::Exchange::Bj) => Ok(Exchange::Bj),
        _ => Err(Status::invalid_argument(format!(
            "invalid exchange: {value}"
        ))),
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
fn direction_from_i32(value: i32) -> std::result::Result<SignalDirection, Status> {
    match pb::SignalDirection::from_i32(value) {
        Some(pb::SignalDirection::Long) => Ok(SignalDirection::Long),
        Some(pb::SignalDirection::Short) => Ok(SignalDirection::Short),
        Some(pb::SignalDirection::Flat) => Ok(SignalDirection::Flat),
        Some(pb::SignalDirection::CloseLong) => Ok(SignalDirection::CloseLong),
        Some(pb::SignalDirection::CloseShort) => Ok(SignalDirection::CloseShort),
        _ => Err(Status::invalid_argument(format!(
            "invalid signal direction: {value}"
        ))),
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
fn style_from_i32(value: i32) -> std::result::Result<StrategyStyle, Status> {
    match pb::StrategyStyle::from_i32(value) {
        Some(pb::StrategyStyle::Swing) => Ok(StrategyStyle::Swing),
        Some(pb::StrategyStyle::T0) => Ok(StrategyStyle::T0),
        Some(pb::StrategyStyle::LimitUp) => Ok(StrategyStyle::LimitUp),
        _ => Err(Status::invalid_argument(format!(
            "invalid strategy style: {value}"
        ))),
    }
}

fn action_to_proto(action: DecisionAction) -> pb::DecisionAction {
    match action {
        DecisionAction::Open => pb::DecisionAction::Open,
        DecisionAction::Add => pb::DecisionAction::Add,
        DecisionAction::Reduce => pb::DecisionAction::Reduce,
        DecisionAction::Close => pb::DecisionAction::Close,
        DecisionAction::Hold => pb::DecisionAction::Hold,
    }
}

fn side_to_proto(side: OrderSide) -> pb::OrderSide {
    match side {
        OrderSide::Buy => pb::OrderSide::Buy,
        OrderSide::Sell => pb::OrderSide::Sell,
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type.
fn utc_from_ms(ms: i64) -> std::result::Result<DateTime<Utc>, Status> {
    Utc.timestamp_millis_opt(ms)
        .single()
        .ok_or_else(|| Status::invalid_argument(format!("invalid epoch millis: {ms}")))
}

fn status_from_error(error: anyhow::Error) -> Status {
    match error.downcast_ref::<TgError>() {
        Some(TgError::Validation(message)) => Status::invalid_argument(message.clone()),
        Some(TgError::NotFound(message)) => Status::not_found(message.clone()),
        Some(TgError::RateLimited) => Status::resource_exhausted("rate limited"),
        Some(TgError::Upstream(message)) => Status::unavailable(message.clone()),
        Some(TgError::InvalidOrder(message)) => Status::failed_precondition(message.clone()),
        Some(TgError::RiskRejected(message)) => Status::failed_precondition(message.clone()),
        Some(TgError::Other(_)) | None => Status::internal(error.to_string()),
    }
}
