use async_trait::async_trait;
use tg_contracts::Bar;

use crate::error::Result;
use crate::factor::{DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta};
use crate::factors::{amount, close, meta};

#[derive(Debug, Clone)]
pub struct AmountPriceValueProxy {
    meta: FactorMeta,
}

impl AmountPriceValueProxy {
    pub fn new() -> Self {
        Self {
            meta: meta(
                "amount_price_value_proxy",
                FactorCategory::Value,
                "Approximate value/liquidity factor: amount / close. This is traded-share turnover value, not PE/PB, used only until fundamentals are available.",
                vec![
                    DataDependency::DailyBars(1),
                    DataDependency::Fundamentals("PE/PB unavailable in Phase 1".to_owned()),
                ],
                &[],
                FactorDirection::Positive,
                true,
            ),
        }
    }
}

impl Default for AmountPriceValueProxy {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Factor for AmountPriceValueProxy {
    fn meta(&self) -> &FactorMeta {
        &self.meta
    }

    async fn compute_timeseries(&self, history: &[Bar]) -> Result<Vec<f64>> {
        Ok(history
            .iter()
            .map(|bar| {
                let price = close(bar);
                if price > 0.0 {
                    amount(bar) / price
                } else {
                    f64::NAN
                }
            })
            .collect())
    }
}
