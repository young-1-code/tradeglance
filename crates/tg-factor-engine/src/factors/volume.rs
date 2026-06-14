use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{ensure_period, meta, nan_series};

#[derive(Debug, Clone)]
pub struct VolumeRatio {
    short: usize,
    long: usize,
    meta: FactorMeta,
}

impl VolumeRatio {
    pub fn new(short: usize, long: usize) -> Self {
        ensure_period("volume_ratio short", short).expect("static period is valid");
        ensure_period("volume_ratio long", long).expect("static period is valid");
        assert!(short <= long, "short period must not exceed long period");
        Self {
            short,
            long,
            meta: meta(
                &format!("volume_ratio_{short}d"),
                FactorCategory::Volume,
                "Volume expansion: mean(volume, short) / mean(volume, long). Useful for A-share short-term liquidity confirmation.",
                vec![DataDependency::DailyBars(long)],
                &[("short", short as f64), ("long", long as f64)],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

#[async_trait]
impl Factor for VolumeRatio {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        let mut out = nan_series(history.len());
        for index in self.long - 1..history.len() {
            let short_start = index + 1 - self.short;
            let long_start = index + 1 - self.long;
            let short_sum = history[short_start..=index]
                .iter()
                .map(|bar| bar.volume as f64)
                .sum::<f64>();
            let long_sum = history[long_start..=index]
                .iter()
                .map(|bar| bar.volume as f64)
                .sum::<f64>();
            if long_sum > 0.0 {
                out[index] = (short_sum / self.short as f64) / (long_sum / self.long as f64);
            }
        }
        Ok(out)
    }
}
