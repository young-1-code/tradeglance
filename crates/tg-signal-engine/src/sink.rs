use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tg_contracts::{OrderId, OrderIntent, Result, Signal, StrategyStyle, TgError};
use tg_engine::OrderSink;
use tokio::sync::broadcast;

#[derive(Debug, Default)]
pub struct NoopSink {
    next_id: AtomicU64,
}

impl NoopSink {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl OrderSink for NoopSink {
    async fn submit(&self, _intent: OrderIntent) -> std::result::Result<OrderId, TgError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(format!("noop-order-{id:020}"))
    }

    async fn cancel(&self, _order_id: &OrderId) -> std::result::Result<(), TgError> {
        Ok(())
    }
}

#[async_trait]
pub trait SignalSink: Send + Sync {
    async fn publish(&self, signal: Signal) -> Result<()>;
}

#[derive(Debug)]
pub struct SignalCollector {
    tx: broadcast::Sender<Signal>,
    buffer: RwLock<VecDeque<Signal>>,
    capacity: usize,
}

impl SignalCollector {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity.max(1));
        Self {
            tx,
            buffer: RwLock::new(VecDeque::with_capacity(capacity.max(1))),
            capacity: capacity.max(1),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
        self.tx.subscribe()
    }

    pub fn query(
        &self,
        symbols: &[String],
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        style: Option<StrategyStyle>,
        limit: Option<usize>,
    ) -> Vec<Signal> {
        let mut out = self
            .buffer
            .read()
            .expect("signal collector lock should not be poisoned")
            .iter()
            .filter(|signal| symbols.is_empty() || symbols.contains(&signal.symbol))
            .filter(|signal| start.map_or(true, |start| signal.ts >= start))
            .filter(|signal| end.map_or(true, |end| signal.ts <= end))
            .filter(|signal| style.map_or(true, |style| signal.style == style))
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|signal| signal.ts);
        if let Some(limit) = limit {
            out.truncate(limit);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.buffer
            .read()
            .expect("signal collector lock should not be poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait]
impl SignalSink for SignalCollector {
    async fn publish(&self, signal: Signal) -> Result<()> {
        {
            let mut buffer = self
                .buffer
                .write()
                .map_err(|_| TgError::Other(anyhow::anyhow!("signal collector lock poisoned")))?;
            if buffer.len() == self.capacity {
                buffer.pop_front();
            }
            buffer.push_back(signal.clone());
        }
        let _ = self.tx.send(signal);
        Ok(())
    }
}

#[derive(Clone)]
pub struct BroadcastSignalSink {
    collector: Arc<SignalCollector>,
}

impl BroadcastSignalSink {
    pub fn new(collector: Arc<SignalCollector>) -> Self {
        Self { collector }
    }
}

#[async_trait]
impl SignalSink for BroadcastSignalSink {
    async fn publish(&self, signal: Signal) -> Result<()> {
        self.collector.publish(signal).await
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use tg_contracts::{
        Exchange, OrderIntent, OrderSide, OrderType, StrategyStyle, TimeInForce,
    };
    use tg_engine::OrderSink;

    use super::*;

    #[tokio::test]
    async fn noop_sink_returns_id_without_side_effects() {
        let sink = NoopSink::new();
        let intent = OrderIntent {
            client_order_id: "client-1".to_owned(),
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            price: Some(Decimal::new(1000, 2)),
            quantity: 100,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
        };

        let id = sink.submit(intent).await.expect("noop submit succeeds");

        assert!(id.starts_with("noop-order-"));
        assert!(sink.cancel(&id).await.is_ok());
    }
}
