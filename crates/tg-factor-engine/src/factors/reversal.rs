use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{close, ensure_period, meta, nan_series};

#[derive(Debug, Clone)]
pub struct ReversalReturn {
    period: usize,
    meta: FactorMeta,
}

impl ReversalReturn {
    pub fn new(period: usize) -> Self {
        ensure_period("reversal", period).expect("static period is valid");
        Self {
            period,
            meta: meta(
                &format!("reversal_{period}d"),
                FactorCategory::Reversal,
                "Short-horizon reversal: -(close[t] / close[t-N] - 1). Positive values indicate recent weakness that may mean-revert.",
                vec![DataDependency::DailyBars(period + 1)],
                &[("period", period as f64)],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

#[async_trait]
impl Factor for ReversalReturn {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let mut out = nan_series(history.len());
        for index in self.period..history.len() {
            let base = close(&history[index - self.period]);
            let current = close(&history[index]);
            if base > 0.0 && current.is_finite() {
                out[index] = -(current / base - 1.0);
            }
        }
        Ok(out)
    }
}
