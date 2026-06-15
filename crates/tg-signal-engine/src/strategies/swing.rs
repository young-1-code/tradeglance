use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Duration;
use tg_contracts::{
    Bar, Fill, Result, Signal, SignalDirection, StrategyStyle, TgError,
};
use tg_engine::{Strategy, StrategyContext};

use crate::rules::{CmpOp, Condition, ConditionTree, EvalContext, Rule};
use crate::sink::SignalSink;
use crate::sources::{FactorSource, FactorValueRequest, IndicatorSeriesRequest, IndicatorSource};

#[derive(Debug, Clone)]
pub struct SwingConfig {
    pub rsi_threshold: f64,
    pub momentum_top_pct: f64,
    pub momentum_universe: u32,
    pub suggested_quantity: i64,
    pub max_history: usize,
}

impl Default for SwingConfig {
    fn default() -> Self {
        Self {
            rsi_threshold: 50.0,
            momentum_top_pct: 0.2,
            momentum_universe: 100,
            suggested_quantity: 100,
            max_history: 128,
        }
    }
}

pub struct SwingStrategy {
    indicators: Arc<dyn IndicatorSource>,
    factors: Arc<dyn FactorSource>,
    signal_sink: Arc<dyn SignalSink>,
    config: SwingConfig,
    bars: Vec<Bar>,
}

impl SwingStrategy {
    pub fn new(
        indicators: Arc<dyn IndicatorSource>,
        factors: Arc<dyn FactorSource>,
        signal_sink: Arc<dyn SignalSink>,
        config: SwingConfig,
    ) -> Self {
        Self {
            indicators,
            factors,
            signal_sink,
            config,
            bars: Vec::new(),
        }
    }

    async fn evaluate_bar(&self, bar: &Bar) -> Result<Option<Signal>> {
        let macd = self
            .indicators
            .fetch_indicator_series(IndicatorSeriesRequest {
                symbol: bar.symbol.clone(),
                period: bar.period,
                indicator: "MACD".to_owned(),
                params: HashMap::from([
                    ("fast".to_owned(), 12.0),
                    ("slow".to_owned(), 26.0),
                    ("signal".to_owned(), 9.0),
                ]),
                bars: self.bars.clone(),
            })
            .await?;
        let rsi = self
            .indicators
            .fetch_indicator_series(IndicatorSeriesRequest {
                symbol: bar.symbol.clone(),
                period: bar.period,
                indicator: "RSI".to_owned(),
                params: HashMap::from([("period".to_owned(), 14.0)]),
                bars: self.bars.clone(),
            })
            .await?;
        let factors = self
            .factors
            .fetch_factor_values(FactorValueRequest {
                factor: "momentum_20d".to_owned(),
                symbols: vec![bar.symbol.clone()],
                start: bar.ts - Duration::days(30),
                end: bar.ts,
            })
            .await?;

        let dif = macd
            .series
            .get("dif")
            .ok_or_else(|| TgError::Validation("MACD missing dif series".to_owned()))?;
        let dea = macd
            .series
            .get("dea")
            .ok_or_else(|| TgError::Validation("MACD missing dea series".to_owned()))?;
        if dif.len() < 2 || dea.len() < 2 {
            return Ok(None);
        }

        let prev_spread = dif[dif.len() - 2] - dea[dea.len() - 2];
        let current_spread = dif[dif.len() - 1] - dea[dea.len() - 1];
        let macd_cross = if prev_spread <= 0.0 && current_spread > 0.0 {
            1.0
        } else {
            0.0
        };
        let rsi_latest = match rsi.latest("rsi") {
            Some(value) => value,
            None => return Ok(None),
        };
        let factor = match factors.into_iter().max_by_key(|value| value.ts) {
            Some(value) => value,
            None => return Ok(None),
        };

        let ctx = EvalContext::default()
            .with_indicator("MACD_GOLDEN_CROSS", macd_cross)
            .with_indicator("RSI14", rsi_latest)
            .with_factor(factor);
        let rule = Rule {
            id: "swing-long".to_owned(),
            style: StrategyStyle::Swing,
            direction: SignalDirection::Long,
            condition: ConditionTree::And(vec![
                ConditionTree::Leaf(Condition::IndicatorThreshold {
                    key: "MACD_GOLDEN_CROSS".to_owned(),
                    op: CmpOp::Gt,
                    threshold: 0.5,
                    reason_code: "MACD golden cross".to_owned(),
                }),
                ConditionTree::Leaf(Condition::IndicatorThreshold {
                    key: "RSI14".to_owned(),
                    op: CmpOp::Lt,
                    threshold: self.config.rsi_threshold,
                    reason_code: "RSI(14)".to_owned(),
                }),
                ConditionTree::Leaf(Condition::FactorRankThreshold {
                    factor: "momentum_20d".to_owned(),
                    max_rank_pct: self.config.momentum_top_pct,
                    universe: self.config.momentum_universe,
                    reason_code: "momentum_20d".to_owned(),
                }),
            ]),
        };
        let result = rule.evaluate(&ctx);
        if !result.fired {
            return Ok(None);
        }

        Ok(Some(Signal {
            id: super::next_signal_id("swing"),
            symbol: bar.symbol.clone(),
            exchange: bar.exchange,
            direction: SignalDirection::Long,
            strength: result.strength.clamp(0.0, 1.0),
            confidence: result.confidence.clamp(0.0, 1.0),
            style: StrategyStyle::Swing,
            reason: result.reasons,
            suggested_quantity: Some(self.config.suggested_quantity),
            ts: bar.ts,
            trading_date: bar.trading_date,
        }))
    }
}

#[async_trait]
impl Strategy for SwingStrategy {
    async fn on_init(&mut self, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        Ok(())
    }

    async fn on_bar(&mut self, bar: &Bar, _ctx: &mut StrategyContext<'_>) -> Result<()> {
        self.bars.push(bar.clone());
        if self.bars.len() > self.config.max_history {
            self.bars.remove(0);
        }
        if let Some(signal) = self.evaluate_bar(bar).await? {
            self.signal_sink.publish(signal).await?;
        }
        Ok(())
    }

    async fn on_snapshot(
        &mut self,
        _snap: &tg_contracts::Snapshot,
        _ctx: &mut StrategyContext<'_>,
    ) -> Result<()> {
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
        StrategyStyle::Swing
    }
}
