use async_trait::async_trait;
use rust_decimal::Decimal;
use tg_contracts::{Bar, Fill, Result, Signal, SignalDirection, Snapshot, StrategyStyle};
use tg_engine::{Strategy, StrategyContext};

use crate::sink::SignalSink;

#[derive(Debug, Clone)]
pub struct LimitUpConfig {
    pub limit_up_pct: Decimal,
    pub seal_ratio: i64,
    pub min_bid_volume: i64,
    pub suggested_quantity: i64,
}

impl Default for LimitUpConfig {
    fn default() -> Self {
        Self {
            limit_up_pct: Decimal::new(10, 2),
            seal_ratio: 10,
            min_bid_volume: 100_000,
            suggested_quantity: 100,
        }
    }
}

pub struct LimitUpStrategy {
    signal_sink: std::sync::Arc<dyn SignalSink>,
    config: LimitUpConfig,
}

impl LimitUpStrategy {
    pub fn new(signal_sink: std::sync::Arc<dyn SignalSink>, config: LimitUpConfig) -> Self {
        Self {
            signal_sink,
            config,
        }
    }

    fn is_sealed(&self, snap: &Snapshot) -> Option<(Decimal, i64, i64)> {
        let limit_up = (snap.pre_close * (Decimal::ONE + self.config.limit_up_pct)).round_dp(2);
        if snap.last != limit_up {
            return None;
        }
        let bid1 = snap.bid_volume[0];
        let ask1 = snap.ask_volume[0];
        let ratio_pass = if ask1 <= 0 {
            bid1 >= self.config.min_bid_volume
        } else {
            bid1 >= ask1.saturating_mul(self.config.seal_ratio)
        };
        (bid1 >= self.config.min_bid_volume && ratio_pass).then_some((limit_up, bid1, ask1))
    }
}

#[async_trait]
impl Strategy for LimitUpStrategy {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_bar(&mut self, _bar: &Bar, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_snapshot(&mut self, snap: &Snapshot, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        // Limit-up board trading depends on seconds-level order-book persistence. Without that
        // history, backtest results are not reliable; this strategy is wired for live/paper use.
        if let Some((limit_up, bid1, ask1)) = self.is_sealed(snap) {
            self.signal_sink
                .publish(Signal {
                    id: super::next_signal_id("limitup"),
                    symbol: snap.symbol.clone(),
                    exchange: snap.exchange,
                    direction: SignalDirection::Long,
                    strength: 0.85,
                    confidence: 0.55,
                    style: StrategyStyle::LimitUp,
                    reason: vec![
                        format!("price:sealed at limit_up last={} limit_up={limit_up}", snap.last),
                        format!("orderbook:bid1={bid1} ask1={ask1} solid seal"),
                        "timing:tail-of-day buy intent; next-day sell handled by decision/execution"
                            .to_owned(),
                    ],
                    suggested_quantity: Some(self.config.suggested_quantity),
                    ts: snap.ts,
                    trading_date: snap.trading_date,
                })
                .await?;
        }
        Ok(())
    }

    async fn on_timer(
        &mut self,
        _at: chrono::DateTime<chrono::Utc>,
        _ctx: &mut StrategyContext<'_>,
    ) -> Result<()> {
        Ok(())
    }

    async fn on_fill(&mut self, _fill: &Fill, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_shutdown(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    fn style(&self) -> StrategyStyle {
        StrategyStyle::LimitUp
    }
}
