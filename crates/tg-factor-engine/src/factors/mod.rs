use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use tg_contracts::Bar;

use crate::error::{FactorError, Result};
use crate::factor::{DataDependency, FactorCategory, FactorDirection, FactorMeta, FactorRegistry};

pub mod momentum;
pub mod moneyflow;
pub mod reversal;
pub mod rsi;
pub mod size;
pub mod value;
pub mod volatility;
pub mod volume;

pub use momentum::MomentumReturn;
pub use moneyflow::{AmountMomentum, ObvMoneyFlow};
pub use reversal::ReversalReturn;
pub use rsi::RsiFactor;
pub use size::LogAmountSizeProxy;
pub use value::AmountPriceValueProxy;
pub use volatility::RealizedVolatility;
pub use volume::VolumeRatio;

pub(crate) fn meta(
    name: &str,
    category: FactorCategory,
    logic: &str,
    data_dependencies: Vec<DataDependency>,
    params: &[(&str, f64)],
    direction: FactorDirection,
    enabled: bool,
) -> FactorMeta {
    FactorMeta {
        name: name.to_owned(),
        category,
        logic: logic.to_owned(),
        data_dependencies,
        params: params
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect::<HashMap<_, _>>(),
        direction,
        enabled,
    }
}

pub(crate) fn close(bar: &Bar) -> f64 {
    bar.close.to_f64().unwrap_or(f64::NAN)
}

pub(crate) fn amount(bar: &Bar) -> f64 {
    bar.amount.to_f64().unwrap_or(f64::NAN)
}

pub(crate) fn nan_series(len: usize) -> Vec<f64> {
    vec![f64::NAN; len]
}

pub(crate) fn ensure_period(name: &str, period: usize) -> Result<()> {
    if period == 0 {
        return Err(FactorError::InvalidInput(format!(
            "{name} period must be positive"
        )));
    }
    Ok(())
}

pub(crate) fn simple_returns(history: &[Bar]) -> Vec<f64> {
    let mut returns = vec![f64::NAN; history.len()];
    for index in 1..history.len() {
        let prev = close(&history[index - 1]);
        let curr = close(&history[index]);
        if prev > 0.0 && curr.is_finite() {
            returns[index] = curr / prev - 1.0;
        }
    }
    returns
}

pub fn default_registry() -> FactorRegistry {
    let mut registry = FactorRegistry::new();
    for result in [
        registry.register(MomentumReturn::new(20)),
        registry.register(MomentumReturn::new(60)),
        registry.register(ReversalReturn::new(5)),
        registry.register(RealizedVolatility::new(20)),
        registry.register(VolumeRatio::new(5, 20)),
        registry.register(AmountMomentum::new(5, 20)),
        registry.register(ObvMoneyFlow::new()),
        registry.register(LogAmountSizeProxy::new()),
        registry.register(AmountPriceValueProxy::new()),
        registry.register(RsiFactor::new(14)),
    ] {
        result.expect("default factor names are unique");
    }
    registry
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{Bar, BarPeriod, Exchange};

    use super::{
        AmountMomentum, AmountPriceValueProxy, LogAmountSizeProxy, MomentumReturn, ObvMoneyFlow,
        RealizedVolatility, ReversalReturn, RsiFactor, VolumeRatio,
    };
    use crate::factor::Factor;

    fn dec(value: i64) -> Decimal {
        Decimal::new(value, 0)
    }

    fn bars(closes: &[i64], volumes: &[i64], amounts: &[i64]) -> Vec<Bar> {
        let start = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        closes
            .iter()
            .enumerate()
            .map(|(index, close)| {
                let date = start + Duration::days(index as i64);
                Bar {
                    symbol: "600519".to_owned(),
                    exchange: Exchange::Sh,
                    period: BarPeriod::Daily,
                    ts: Utc
                        .with_ymd_and_hms(2026, 6, 1 + index as u32, 7, 0, 0)
                        .unwrap(),
                    trading_date: date,
                    open: dec(*close),
                    high: dec(*close),
                    low: dec(*close),
                    close: dec(*close),
                    volume: volumes[index],
                    amount: dec(amounts[index]),
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn momentum_and_reversal_match_hand_values() {
        let bars = bars(&[10, 11, 12, 15], &[1; 4], &[10; 4]);
        let momentum = MomentumReturn::new(2)
            .compute_timeseries(&bars)
            .await
            .unwrap();
        assert!(momentum[0].is_nan());
        assert!((momentum[2] - 0.2).abs() < 1e-12);
        assert!((momentum[3] - (15.0 / 11.0 - 1.0)).abs() < 1e-12);

        let reversal = ReversalReturn::new(2)
            .compute_timeseries(&bars)
            .await
            .unwrap();
        assert!((reversal[2] + 0.2).abs() < 1e-12);
    }

    #[tokio::test]
    async fn volatility_uses_population_std_of_returns() {
        let bars = bars(&[100, 110, 99], &[1; 3], &[10; 3]);
        let values = RealizedVolatility::new(2)
            .compute_timeseries(&bars)
            .await
            .unwrap();
        assert!((values[2] - 0.1).abs() < 1e-12);
    }

    #[tokio::test]
    async fn volume_and_amount_ratios_match_hand_values() {
        let bars = bars(&[10, 10, 10, 10], &[10, 20, 30, 40], &[100, 200, 300, 400]);
        let volume = VolumeRatio::new(2, 4)
            .compute_timeseries(&bars)
            .await
            .unwrap();
        assert!((volume[3] - 1.4).abs() < 1e-12);

        let amount = AmountMomentum::new(2, 4)
            .compute_timeseries(&bars)
            .await
            .unwrap();
        assert!((amount[3] - 0.7).abs() < 1e-12);
    }

    #[tokio::test]
    async fn moneyflow_size_value_and_rsi_are_deterministic() {
        let bars = bars(
            &[10, 12, 11, 11, 13],
            &[100, 200, 300, 400, 500],
            &[1000, 2400, 3300, 4400, 6500],
        );
        let obv = ObvMoneyFlow::new().compute_timeseries(&bars).await.unwrap();
        assert_eq!(obv, vec![0.0, 200.0, -100.0, -100.0, 400.0]);

        let size = LogAmountSizeProxy::new()
            .compute_timeseries(&bars[0..1])
            .await
            .unwrap();
        assert!((size[0] - 1000.0_f64.ln()).abs() < 1e-12);

        let value = AmountPriceValueProxy::new()
            .compute_timeseries(&bars[0..1])
            .await
            .unwrap();
        assert!((value[0] - 100.0).abs() < 1e-12);

        let rsi = RsiFactor::new(2).compute_timeseries(&bars).await.unwrap();
        assert!(rsi[0].is_nan());
        assert!((rsi[2] - (50.0 - 66.66666666666666)).abs() < 1e-10);
    }
}
