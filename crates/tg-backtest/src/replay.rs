use std::collections::VecDeque;
use std::ops::Range;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tg_contracts::{Adjustment, Bar, BarPeriod, BarQuery, Event, Result};
use tg_engine::DataFeed;
use tg_persistence::BarRepo;

#[derive(Debug, Clone)]
pub struct BacktestReplay {
    bars: VecDeque<Bar>,
}

impl BacktestReplay {
    pub fn from_bars(mut bars: Vec<Bar>) -> Self {
        bars.sort_by(|left, right| {
            left.ts
                .cmp(&right.ts)
                .then_with(|| left.symbol.cmp(&right.symbol))
        });
        Self {
            bars: VecDeque::from(bars),
        }
    }

    pub async fn from_repo(
        repo: &dyn BarRepo,
        universe: &[String],
        period: BarPeriod,
        range: Range<DateTime<Utc>>,
        adjustment: Adjustment,
    ) -> Result<Self> {
        let mut bars = Vec::new();
        for symbol in universe {
            let mut symbol_bars = repo
                .query_bars(BarQuery {
                    symbol: symbol.clone(),
                    period,
                    range: range.clone(),
                    adjustment,
                })
                .await?;
            bars.append(&mut symbol_bars);
        }
        Ok(Self::from_bars(bars))
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }
}

#[async_trait]
impl DataFeed for BacktestReplay {
    async fn next_event(&mut self) -> Result<Option<Event>> {
        Ok(self.bars.pop_front().map(Event::Bar))
    }

    async fn peek_next_ts(&mut self) -> Result<Option<DateTime<Utc>>> {
        Ok(self.bars.front().map(|bar| bar.ts))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{BarPeriod, Exchange};

    use super::*;

    fn bar(symbol: &str, minute: u32) -> Bar {
        Bar {
            symbol: symbol.to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Min1,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 1, minute, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            open: Decimal::new(1000, 2),
            high: Decimal::new(1100, 2),
            low: Decimal::new(900, 2),
            close: Decimal::new(1050, 2),
            volume: 10_000,
            amount: Decimal::new(105_000, 2),
        }
    }

    #[tokio::test]
    async fn replay_merges_multi_symbol_bars_by_ts_then_symbol() {
        let mut replay = BacktestReplay::from_bars(vec![
            bar("600002", 2),
            bar("600002", 1),
            bar("600001", 1),
            bar("600001", 3),
        ]);

        let mut seen = Vec::new();
        while let Some(Event::Bar(bar)) = replay.next_event().await.unwrap() {
            seen.push((bar.ts, bar.symbol));
        }

        assert_eq!(
            seen.into_iter()
                .map(|(_, symbol)| symbol)
                .collect::<Vec<_>>(),
            vec!["600001", "600002", "600002", "600001"]
        );
    }
}
