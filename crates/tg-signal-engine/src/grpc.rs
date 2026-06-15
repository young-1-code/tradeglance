use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use tg_contracts::proto::tg::v1 as pb;
use tg_contracts::{Exchange, Signal, SignalDirection, StrategyStyle};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::sink::SignalCollector;

#[derive(Clone)]
pub struct SignalGrpcService {
    collector: Arc<SignalCollector>,
}

impl SignalGrpcService {
    pub fn new(collector: Arc<SignalCollector>) -> Self {
        Self { collector }
    }
}

#[tonic::async_trait]
impl pb::signal_service_server::SignalService for SignalGrpcService {
    type SubscribeSignalsStream =
        Pin<Box<dyn Stream<Item = std::result::Result<pb::Signal, Status>> + Send + 'static>>;

    async fn subscribe_signals(
        &self,
        request: Request<pb::SubscribeSignalsRequest>,
    ) -> std::result::Result<Response<Self::SubscribeSignalsStream>, Status> {
        let request = request.into_inner();
        let symbols = request.symbols;
        let mut rx = self.collector.subscribe();
        let (tx, out_rx) = mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(signal) => {
                        if !symbols.is_empty() && !symbols.contains(&signal.symbol) {
                            continue;
                        }
                        if tx.send(Ok(signal_to_proto(signal))).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(out_rx))))
    }

    async fn query_signals(
        &self,
        request: Request<pb::QuerySignalsRequest>,
    ) -> std::result::Result<Response<pb::QuerySignalsResponse>, Status> {
        let request = request.into_inner();
        let start = optional_utc_from_ms(request.start_epoch_millis)?;
        let end = optional_utc_from_ms(request.end_epoch_millis)?;
        let signals = self
            .collector
            .query(&request.symbols, start, end, None, None)
            .into_iter()
            .map(signal_to_proto)
            .collect();
        Ok(Response::new(pb::QuerySignalsResponse { signals }))
    }
}

fn signal_to_proto(signal: Signal) -> pb::Signal {
    pb::Signal {
        id: signal.id,
        symbol: signal.symbol,
        exchange: exchange_to_proto(signal.exchange) as i32,
        direction: direction_to_proto(signal.direction) as i32,
        strength: signal.strength,
        confidence: signal.confidence,
        style: style_to_proto(signal.style) as i32,
        reason: signal.reason,
        suggested_quantity: signal.suggested_quantity.unwrap_or_default(),
        has_suggested_quantity: signal.suggested_quantity.is_some(),
        ts_epoch_millis: signal.ts.timestamp_millis(),
        trading_date: signal.trading_date.to_string(),
    }
}

fn exchange_to_proto(exchange: Exchange) -> pb::Exchange {
    match exchange {
        Exchange::Sh => pb::Exchange::Sh,
        Exchange::Sz => pb::Exchange::Sz,
        Exchange::Bj => pb::Exchange::Bj,
    }
}

fn direction_to_proto(direction: SignalDirection) -> pb::SignalDirection {
    match direction {
        SignalDirection::Long => pb::SignalDirection::Long,
        SignalDirection::Short => pb::SignalDirection::Short,
        SignalDirection::Flat => pb::SignalDirection::Flat,
        SignalDirection::CloseLong => pb::SignalDirection::CloseLong,
        SignalDirection::CloseShort => pb::SignalDirection::CloseShort,
    }
}

fn style_to_proto(style: StrategyStyle) -> pb::StrategyStyle {
    match style {
        StrategyStyle::Swing => pb::StrategyStyle::Swing,
        StrategyStyle::T0 => pb::StrategyStyle::T0,
        StrategyStyle::LimitUp => pb::StrategyStyle::LimitUp,
    }
}

#[allow(clippy::result_large_err)] // tonic::Status is the idiomatic gRPC error type
fn optional_utc_from_ms(ms: i64) -> std::result::Result<Option<DateTime<Utc>>, Status> {
    if ms == 0 {
        return Ok(None);
    }
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(Some)
        .ok_or_else(|| Status::invalid_argument(format!("invalid epoch millis: {ms}")))
}
