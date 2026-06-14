use chrono::NaiveDate;
use rust_decimal::Decimal;
use tg_contracts::{
    limit_up_pct, AdjustmentFactor, Bar, Board, Snapshot, TgError, TradingCalendar,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustmentGap {
    pub symbol: String,
    pub date: NaiveDate,
    pub reason: String,
}

pub fn is_price_within_limit(
    high: Decimal,
    low: Decimal,
    pre_close: Decimal,
    board: Board,
) -> bool {
    if pre_close <= Decimal::ZERO {
        return false;
    }
    let pct = limit_up_pct(board);
    let upper = pre_close * (Decimal::ONE + pct);
    let lower = pre_close * (Decimal::ONE - pct);
    high <= upper && low >= lower
}

pub fn validate_bar_limit(bar: &Bar, pre_close: Decimal, board: Board) -> tg_contracts::Result<()> {
    if is_price_within_limit(bar.high, bar.low, pre_close, board) {
        Ok(())
    } else {
        Err(TgError::Validation(format!(
            "{} {} OHLC outside board limit",
            bar.symbol, bar.trading_date
        )))
    }
}

pub fn validate_snapshot_limit(snapshot: &Snapshot, board: Board) -> tg_contracts::Result<()> {
    if is_price_within_limit(snapshot.high, snapshot.low, snapshot.pre_close, board) {
        Ok(())
    } else {
        Err(TgError::Validation(format!(
            "{} {} snapshot outside board limit",
            snapshot.symbol, snapshot.trading_date
        )))
    }
}

pub fn is_suspended_bar(bar: &Bar, pre_close: Decimal) -> bool {
    bar.volume == 0
        && bar.open == pre_close
        && bar.high == pre_close
        && bar.low == pre_close
        && bar.close == pre_close
}

pub fn is_suspended_snapshot(snapshot: &Snapshot) -> bool {
    snapshot.volume == 0
        && snapshot.last == snapshot.pre_close
        && snapshot.open == snapshot.pre_close
        && snapshot.high == snapshot.pre_close
        && snapshot.low == snapshot.pre_close
}

pub fn missing_trading_days(
    calendar: &[TradingCalendar],
    bars: &[Bar],
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<NaiveDate> {
    calendar
        .iter()
        .filter(|day| day.is_trading_day && day.date >= start && day.date <= end)
        .filter(|day| !bars.iter().any(|bar| bar.trading_date == day.date))
        .map(|day| day.date)
        .collect()
}

pub fn flag_adjustment_gaps(
    bars: &[Bar],
    factors: &[AdjustmentFactor],
    threshold_pct: Decimal,
) -> Vec<AdjustmentGap> {
    bars.windows(2)
        .filter_map(|pair| {
            let previous = &pair[0];
            let current = &pair[1];
            if previous.close <= Decimal::ZERO {
                return None;
            }
            let raw_gap = ((current.open - previous.close) / previous.close).abs();
            let has_factor = factors.iter().any(|factor| {
                factor.symbol == current.symbol && factor.ex_date == current.trading_date
            });
            if raw_gap > threshold_pct && has_factor {
                Some(AdjustmentGap {
                    symbol: current.symbol.clone(),
                    date: current.trading_date,
                    reason: "large raw-price gap on adjustment date".to_owned(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{BarPeriod, Exchange};

    use super::*;

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn bar(date: NaiveDate, close: Decimal, volume: i64) -> Bar {
        Bar {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Daily,
            ts: Utc
                .with_ymd_and_hms(date.year(), date.month(), date.day(), 7, 0, 0)
                .unwrap(),
            trading_date: date,
            open: close,
            high: close,
            low: close,
            close,
            volume,
            amount: Decimal::ZERO,
        }
    }

    trait DateParts {
        fn year(&self) -> i32;
        fn month(&self) -> u32;
        fn day(&self) -> u32;
    }

    impl DateParts for NaiveDate {
        fn year(&self) -> i32 {
            chrono::Datelike::year(self)
        }

        fn month(&self) -> u32 {
            chrono::Datelike::month(self)
        }

        fn day(&self) -> u32 {
            chrono::Datelike::day(self)
        }
    }

    #[test]
    fn price_limit_accepts_boundary_and_rejects_over_limit() {
        let pre_close = dec(1000, 2);
        assert!(is_price_within_limit(
            dec(1100, 2),
            dec(900, 2),
            pre_close,
            Board::MainBoard
        ));
        assert!(!is_price_within_limit(
            dec(1101, 2),
            dec(900, 2),
            pre_close,
            Board::MainBoard
        ));
    }

    #[test]
    fn suspended_bar_requires_zero_volume_and_unchanged_price() {
        let day = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        assert!(is_suspended_bar(&bar(day, dec(1000, 2), 0), dec(1000, 2)));
        assert!(!is_suspended_bar(&bar(day, dec(1001, 2), 0), dec(1000, 2)));
        assert!(!is_suspended_bar(
            &bar(day, dec(1000, 2), 100),
            dec(1000, 2)
        ));
    }

    #[test]
    fn missing_bar_detection_uses_trading_calendar() {
        let d1 = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
        let d2 = NaiveDate::from_ymd_opt(2026, 6, 16).unwrap();
        let d3 = NaiveDate::from_ymd_opt(2026, 6, 17).unwrap();
        let calendar = vec![
            TradingCalendar {
                date: d1,
                is_trading_day: true,
            },
            TradingCalendar {
                date: d2,
                is_trading_day: false,
            },
            TradingCalendar {
                date: d3,
                is_trading_day: true,
            },
        ];
        let bars = vec![bar(d1, dec(1000, 2), 100)];

        assert_eq!(missing_trading_days(&calendar, &bars, d1, d3), vec![d3]);
    }
}
