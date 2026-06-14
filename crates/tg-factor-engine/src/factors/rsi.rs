use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{close, ensure_period, meta, nan_series};

#[derive(Debug, Clone)]
pub struct RsiFactor {
    period: usize,
    meta: FactorMeta,
}

impl RsiFactor {
    pub fn new(period: usize) -> Self {
        ensure_period("rsi_factor", period).expect("static period is valid");
        Self {
            period,
            meta: meta(
                &format!("rsi_factor_{period}"),
                FactorCategory::Reversal,
                "RSI reversal factor: 50 - RSI(N). RSI uses Wilder smoothing internally for Phase 1; swap to indicators gRPC when tg-indicators is available.",
                vec![DataDependency::DailyBars(period + 1)],
                &[("period", period as f64)],
                FactorDirection::Positive,
                true,
            ),
        }
    }

    fn rsi_series(&self, history: &[Bar]) -> Vec<f64> {
        let mut rsi = nan_series(history.len());
        if history.len() <= self.period {
            return rsi;
        }

        let mut avg_gain = 0.0;
        let mut avg_loss = 0.0;
        for index in 1..=self.period {
            let diff = close(&history[index]) - close(&history[index - 1]);
            if diff >= 0.0 {
                avg_gain += diff;
            } else {
                avg_loss -= diff;
            }
        }
        avg_gain /= self.period as f64;
        avg_loss /= self.period as f64;
        rsi[self.period] = rsi_from_average(avg_gain, avg_loss);

        for index in self.period + 1..history.len() {
            let diff = close(&history[index]) - close(&history[index - 1]);
            let gain = diff.max(0.0);
            let loss = (-diff).max(0.0);
            avg_gain = (avg_gain * (self.period - 1) as f64 + gain) / self.period as f64;
            avg_loss = (avg_loss * (self.period - 1) as f64 + loss) / self.period as f64;
            rsi[index] = rsi_from_average(avg_gain, avg_loss);
        }
        rsi
    }
}

fn rsi_from_average(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_loss == 0.0 {
        if avg_gain == 0.0 {
            50.0
        } else {
            100.0
        }
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    }
}

#[async_trait]
impl Factor for RsiFactor {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        Ok(self
            .rsi_series(history)
            .into_iter()
            .map(|value| {
                if value.is_finite() {
                    50.0 - value
                } else {
                    f64::NAN
                }
            })
            .collect())
    }
}
