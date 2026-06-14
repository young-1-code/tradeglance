use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{close, ensure_period, meta, nan_series};

#[derive(Debug, Clone)]
pub struct MomentumReturn {
    period: usize,
    meta: FactorMeta,
}

impl MomentumReturn {
    pub fn new(period: usize) -> Self {
        ensure_period("momentum", period).expect("static period is valid");
        Self {
            period,
            meta: meta(
                &format!("momentum_{period}d"),
                FactorCategory::Momentum,
                "N-day return: close[t] / close[t-N] - 1, using pre-adjusted A-share bars when supplied by persistence.",
                vec![DataDependency::DailyBars(period + 1)],
                &[("period", period as f64)],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

#[async_trait]
impl Factor for MomentumReturn {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let mut out = nan_series(history.len());
        for index in self.period..history.len() {
            let base = close(&history[index - self.period]);
            let current = close(&history[index]);
            if base > 0.0 && current.is_finite() {
                out[index] = current / base - 1.0;
            }
        }
        Ok(out)
    }
}
