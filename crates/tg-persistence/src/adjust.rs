use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use tg_contracts::{Adjustment, AdjustmentFactor, Bar, Result, TgError};

/// Applies query-time adjustment to raw bars. Storage remains unadjusted.
pub fn adjust_bars(
    mut bars: Vec<Bar>,
    factors: &[AdjustmentFactor],
    adjustment: Adjustment,
) -> Result<Vec<Bar>> {
    if matches!(adjustment, Adjustment::None) || bars.is_empty() {
        return Ok(bars);
    }

    let Some(anchor) = (match adjustment {
        Adjustment::None => None,
        Adjustment::PreAdjust => factors.iter().map(|factor| factor.factor).next_back(),
        Adjustment::PostAdjust => factors.iter().map(|factor| factor.factor).next(),
    }) else {
        return Ok(bars);
    };

    for bar in &mut bars {
        let Some(factor) = factor_for_bar(bar, factors) else {
            continue;
        };
        if factor.is_zero() {
            return Err(TgError::Validation(format!(
                "zero adjustment factor for {} on {}",
                bar.symbol, bar.trading_date
            )));
        }
        let ratio = factor / anchor;
        scale_bar(bar, ratio)?;
    }

    Ok(bars)
}

fn factor_for_bar(bar: &Bar, factors: &[AdjustmentFactor]) -> Option<Decimal> {
    factors
        .iter()
        .take_while(|factor| factor.ex_date <= bar.trading_date)
        .last()
        .map(|factor| factor.factor)
}

fn scale_bar(bar: &mut Bar, ratio: Decimal) -> Result<()> {
    bar.open *= ratio;
    bar.high *= ratio;
    bar.low *= ratio;
    bar.close *= ratio;

    let adjusted_volume = Decimal::from(bar.volume) / ratio;
    bar.volume = adjusted_volume
        .round()
        .to_i64()
        .ok_or_else(|| TgError::Validation("adjusted volume out of i64 range".to_owned()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{Adjustment, AdjustmentFactor, Bar, BarPeriod, Exchange};

    use super::adjust_bars;

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    fn bar(trading_date: NaiveDate, close: Decimal, volume: i64) -> Bar {
        Bar {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Daily,
            ts: Utc
                .with_ymd_and_hms(
                    trading_date.year(),
                    trading_date.month(),
                    trading_date.day(),
                    7,
                    0,
                    0,
                )
                .unwrap(),
            trading_date,
            open: close,
            high: close,
            low: close,
            close,
            volume,
            amount: close * Decimal::from(volume),
        }
    }

    #[test]
    fn pre_adjusts_prices_and_inverse_scales_volume() {
        let bars = vec![
            bar(date(2026, 1, 2), dec(1000, 2), 1_000),
            bar(date(2026, 1, 3), dec(1100, 2), 2_000),
        ];
        let factors = vec![
            AdjustmentFactor {
                symbol: "600519".to_owned(),
                ex_date: date(2026, 1, 1),
                factor: dec(100, 2),
            },
            AdjustmentFactor {
                symbol: "600519".to_owned(),
                ex_date: date(2026, 1, 3),
                factor: dec(200, 2),
            },
        ];

        let adjusted = adjust_bars(bars, &factors, Adjustment::PreAdjust).expect("adjust bars");

        assert_eq!(adjusted[0].close, dec(500, 2));
        assert_eq!(adjusted[0].volume, 2_000);
        assert_eq!(adjusted[1].close, dec(1100, 2));
        assert_eq!(adjusted[1].volume, 2_000);
    }
}
