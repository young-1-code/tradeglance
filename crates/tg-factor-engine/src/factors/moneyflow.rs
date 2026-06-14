use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{amount, close, ensure_period, meta, nan_series};

#[derive(Debug, Clone)]
pub struct AmountMomentum {
    short: usize,
    long: usize,
    meta: FactorMeta,
}

impl AmountMomentum {
    pub fn new(short: usize, long: usize) -> Self {
        ensure_period("amount_momentum short", short).expect("static period is valid");
        ensure_period("amount_momentum long", long).expect("static period is valid");
        assert!(short <= long, "short period must not exceed long period");
        Self {
            short,
            long,
            meta: meta(
                &format!("amount_momentum_{short}d"),
                FactorCategory::MoneyFlow,
                "Amount flow strength: sum(amount, short) / sum(amount, long). Amount is a practical A-share cash-flow proxy when tick-level capital flow is unavailable.",
                vec![DataDependency::DailyBars(long)],
                &[("short", short as f64), ("long", long as f64)],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

#[async_trait]
impl Factor for AmountMomentum {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let mut out = nan_series(history.len());
        for index in self.long - 1..history.len() {
            let short_sum = history[index + 1 - self.short..=index]
                .iter()
                .map(amount)
                .sum::<f64>();
            let long_sum = history[index + 1 - self.long..=index]
                .iter()
                .map(amount)
                .sum::<f64>();
            if long_sum > 0.0 {
                out[index] = short_sum / long_sum;
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone)]
pub struct ObvMoneyFlow {
    meta: FactorMeta,
}

impl ObvMoneyFlow {
    pub fn new() -> Self {
        Self {
            meta: meta(
                "obv_moneyflow",
                FactorCategory::MoneyFlow,
                "OBV-style money flow: cumulative signed volume, adding volume on up closes and subtracting on down closes.",
                vec![DataDependency::DailyBars(2)],
                &[],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

impl Default for ObvMoneyFlow {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Factor for ObvMoneyFlow {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let mut out = vec![0.0; history.len()];
        for index in 1..history.len() {
            let prev = close(&history[index - 1]);
            let curr = close(&history[index]);
            let signed_volume = if curr > prev {
                history[index].volume as f64
            } else if curr < prev {
                -(history[index].volume as f64)
            } else {
                0.0
            };
            out[index] = out[index - 1] + signed_volume;
        }
        Ok(out)
    }
}
