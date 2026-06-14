use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{ensure_period, meta, nan_series, simple_returns};

#[derive(Debug, Clone)]
pub struct RealizedVolatility {
    period: usize,
    meta: FactorMeta,
}

impl RealizedVolatility {
    pub fn new(period: usize) -> Self {
        ensure_period("volatility", period).expect("static period is valid");
        Self {
            period,
            meta: meta(
                &format!("volatility_{period}d"),
                FactorCategory::Volatility,
                "Realized volatility: population standard deviation of daily simple returns over N observations. Higher values are treated as lower quality for ranking.",
                vec![DataDependency::DailyBars(period + 1)],
                &[("period", period as f64)],
                FactorDirection::Negative,
                true,
            ),
        }
    }
}

#[async_trait]
impl Factor for RealizedVolatility {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let returns = simple_returns(history);
        let mut out = nan_series(history.len());
        for index in self.period..history.len() {
            let window = &returns[index + 1 - self.period..=index];
            if window.iter().all(|value| value.is_finite()) {
                let mean = window.iter().sum::<f64>() / self.period as f64;
                let var = window
                    .iter()
                    .map(|value| (value - mean).powi(2))
                    .sum::<f64>()
                    / self.period as f64;
                out[index] = var.sqrt();
            }
        }
        Ok(out)
    }
}
