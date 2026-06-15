use async_trait::async_trait;
use rust_decimal::Decimal;
use tg_contracts::{Bar, Fill, Position, Result, Signal, SignalDirection, Snapshot, StrategyStyle};
use tg_engine::{Strategy, StrategyContext};

use crate::sink::SignalSink;

#[derive(Debug, Clone)]
pub struct T0Config {
    pub sell_above_cost_pct: Decimal,
    pub buy_below_cost_pct: Decimal,
    pub lot_quantity: i64,
}

impl Default for T0Config {
    fn default() -> Self {
        Self {
            sell_above_cost_pct: Decimal::new(1, 2),
            buy_below_cost_pct: Decimal::new(1, 2),
            lot_quantity: 100,
        }
    }
}

pub struct T0Strategy {
    signal_sink: std::sync::Arc<dyn SignalSink>,
    config: T0Config,
}

impl T0Strategy {
    pub fn new(signal_sink: std::sync::Arc<dyn SignalSink>, config: T0Config) -> Self {
        Self {
            signal_sink,
            config,
        }
    }

    async fn maybe_emit(
        &self,
        symbol: &str,
        exchange: tg_contracts::Exchange,
        price: Decimal,
        ts: chrono::DateTime<chrono::Utc>,
        trading_date: chrono::NaiveDate,
        position: Option<Position>,
    ) -> Result<()> {
        let Some(position) = position else {
            return Ok(());
        };
        if position.available_quantity <= 0 || position.avg_cost <= Decimal::ZERO {
            return Ok(());
        }

        let sell_level = position.avg_cost * (Decimal::ONE + self.config.sell_above_cost_pct);
        let buy_level = position.avg_cost * (Decimal::ONE - self.config.buy_below_cost_pct);
        let quantity = position
            .available_quantity
            .min(self.config.lot_quantity)
            .max(0);

        let signal = if price >= sell_level {
            Some(Signal {
                id: super::next_signal_id("t0"),
                symbol: symbol.to_owned(),
                exchange,
                direction: SignalDirection::CloseLong,
                strength: 0.75,
                confidence: 0.65,
                style: StrategyStyle::T0,
                reason: vec![
                    format!("price:T0 mean reversion price={price} >= sell_level={sell_level}"),
                    format!(
                        "position:available={} (existing lot)",
                        position.available_quantity
                    ),
                ],
                suggested_quantity: Some(quantity),
                ts,
                trading_date,
            })
        } else if price <= buy_level {
            Some(Signal {
                id: super::next_signal_id("t0"),
                symbol: symbol.to_owned(),
                exchange,
                direction: SignalDirection::Long,
                strength: 0.7,
                confidence: 0.6,
                style: StrategyStyle::T0,
                reason: vec![
                    format!("price:T0 mean reversion price={price} <= buy_level={buy_level}"),
                    format!(
                        "position:available={} (existing lot)",
                        position.available_quantity
                    ),
                ],
                suggested_quantity: Some(quantity),
                ts,
                trading_date,
            })
        } else {
            None
        };

        if let Some(signal) = signal {
            self.signal_sink.publish(signal).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Strategy for T0Strategy {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_bar(&mut self, bar: &Bar, ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.maybe_emit(
            &bar.symbol,
            bar.exchange,
            bar.close,
            bar.ts,
            bar.trading_date,
            ctx.portfolio.position(&bar.symbol),
        )
        .await
    }

    async fn on_snapshot(&mut self, snap: &Snapshot, ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.maybe_emit(
            &snap.symbol,
            snap.exchange,
            snap.last,
            snap.ts,
            snap.trading_date,
            ctx.portfolio.position(&snap.symbol),
        )
        .await
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
        StrategyStyle::T0
    }
}
