use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{amount, meta};

#[derive(Debug, Clone)]
pub struct LogAmountSizeProxy {
    meta: FactorMeta,
}

impl LogAmountSizeProxy {
    pub fn new() -> Self {
        Self {
            meta: meta(
                "log_amount_size_proxy",
                FactorCategory::Size,
                "Approximate size factor: ln(amount). This is a Phase 1 liquidity/size proxy until total shares and market-cap snapshots are available.",
                vec![DataDependency::DailyBars(1)],
                &[],
                FactorDirection::Negative,
                true,
            ),
        }
    }
}

impl Default for LogAmountSizeProxy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Factor for LogAmountSizeProxy {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        Ok(history
            .iter()
            .map(|bar| {
                let value = amount(bar);
                if value > 0.0 {
                    value.ln()
                } else {
                    f64::NAN
                }
            })
            .collect())
    }
}
