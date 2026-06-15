use std::collections::HashMap;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{Account, Exchange, Fill, OrderSide, Position, TgError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PositionLot {
    pub symbol: String,
    pub exchange: Exchange,
    pub trading_date: NaiveDate,
    pub quantity: i64,
    pub avg_cost: Decimal,
    pub t0_eligible: bool,
}

#[derive(Debug, Clone)]
pub struct VirtualAccount {
    cash: Decimal,
    frozen_cash: Decimal,
    lots: HashMap<String, Vec<PositionLot>>,
    last_prices: HashMap<String, Decimal>,
    reserved_sell: HashMap<String, i64>,
}

impl VirtualAccount {
    pub fn new(initial_cash: Decimal) -> Self {
        Self {
            cash: initial_cash,
            frozen_cash: Decimal::ZERO,
            lots: HashMap::new(),
            last_prices: HashMap::new(),
            reserved_sell: HashMap::new(),
        }
    }

    pub fn cash(&self) -> Decimal {
        self.cash
    }

    pub fn frozen_cash(&self) -> Decimal {
        self.frozen_cash
    }

    pub fn available_cash(&self) -> Decimal {
        self.cash - self.frozen_cash
    }

    pub fn freeze_cash(&mut self, amount: Decimal) -> Result<(), TgError> {
        if amount < Decimal::ZERO {
            return Err(TgError::InvalidOrder(
                "cannot freeze negative cash".to_owned(),
            ));
        }
        if self.available_cash() < amount {
            return Err(TgError::RiskRejected("insufficient cash".to_owned()));
        }
        self.frozen_cash += amount;
        Ok(())
    }

    pub fn release_cash(&mut self, amount: Decimal) {
        self.frozen_cash = (self.frozen_cash - amount).max(Decimal::ZERO);
    }

    pub fn reserve_sell(
        &mut self,
        symbol: &str,
        quantity: i64,
        today: NaiveDate,
    ) -> Result<(), TgError> {
        if self.available_to_reserve(symbol, today) < quantity {
            return Err(TgError::RiskRejected(
                "insufficient available position".to_owned(),
            ));
        }
        *self.reserved_sell.entry(symbol.to_owned()).or_default() += quantity;
        Ok(())
    }

    pub fn release_sell(&mut self, symbol: &str, quantity: i64) {
        let entry = self.reserved_sell.entry(symbol.to_owned()).or_default();
        *entry = (*entry - quantity).max(0);
    }

    pub fn reserved_sell_quantity(&self, symbol: &str) -> i64 {
        self.reserved_sell.get(symbol).copied().unwrap_or_default()
    }

    pub fn available_to_reserve(&self, symbol: &str, today: NaiveDate) -> i64 {
        self.position(symbol, today)
            .map(|position| position.available_quantity)
            .unwrap_or_default()
            .saturating_sub(self.reserved_sell_quantity(symbol))
    }

    pub fn apply_fill(&mut self, fill: &Fill, t0_eligible: bool) -> Result<(), TgError> {
        let notional = fill.price * Decimal::from(fill.quantity);
        let fees = fill.commission + fill.tax + fill.transfer_fee;
        self.last_prices.insert(fill.symbol.clone(), fill.price);

        match fill.side {
            OrderSide::Buy => {
                self.cash -= notional + fees;
                let avg_cost = (notional + fees) / Decimal::from(fill.quantity);
                self.lots
                    .entry(fill.symbol.clone())
                    .or_default()
                    .push(PositionLot {
                        symbol: fill.symbol.clone(),
                        exchange: fill.exchange,
                        trading_date: fill.trading_date,
                        quantity: fill.quantity,
                        avg_cost,
                        t0_eligible,
                    });
            }
            OrderSide::Sell => {
                self.cash += notional - fees;
                self.release_sell(&fill.symbol, fill.quantity);
                self.consume_lots_fifo(&fill.symbol, fill.quantity, fill.trading_date)?;
            }
        }
        Ok(())
    }

    pub fn add_lot(&mut self, lot: PositionLot) {
        self.last_prices
            .entry(lot.symbol.clone())
            .or_insert(lot.avg_cost);
        self.lots.entry(lot.symbol.clone()).or_default().push(lot);
    }

    pub fn update_market(&mut self, symbol: &str, last_price: Decimal) {
        self.last_prices.insert(symbol.to_owned(), last_price);
    }

    pub fn position(&self, symbol: &str, today: NaiveDate) -> Option<Position> {
        self.aggregate(symbol, self.lots.get(symbol)?, today)
    }

    pub fn positions(&self, today: NaiveDate) -> Vec<Position> {
        let mut positions: Vec<_> = self
            .lots
            .iter()
            .filter_map(|(symbol, lots)| self.aggregate(symbol, lots, today))
            .collect();
        positions.sort_by(|left, right| left.symbol.cmp(&right.symbol));
        positions
    }

    pub fn account(&self, today: NaiveDate) -> Account {
        let positions: HashMap<_, _> = self
            .positions(today)
            .into_iter()
            .map(|position| (position.symbol.clone(), position))
            .collect();
        let positions_value = positions
            .values()
            .fold(Decimal::ZERO, |acc, position| acc + position.market_value);
        Account {
            cash: self.cash,
            frozen_cash: self.frozen_cash,
            total_value: self.cash + positions_value,
            positions,
        }
    }

    fn aggregate(&self, symbol: &str, lots: &[PositionLot], today: NaiveDate) -> Option<Position> {
        let live_lots: Vec<&PositionLot> = lots.iter().filter(|lot| lot.quantity > 0).collect();
        if live_lots.is_empty() {
            return None;
        }
        let total_quantity: i64 = live_lots.iter().map(|lot| lot.quantity).sum();
        let t1_locked_quantity: i64 = live_lots
            .iter()
            .filter(|lot| lot.trading_date == today && !lot.t0_eligible)
            .map(|lot| lot.quantity)
            .sum();
        let cost_basis = live_lots.iter().fold(Decimal::ZERO, |acc, lot| {
            acc + lot.avg_cost * Decimal::from(lot.quantity)
        });
        let avg_cost = cost_basis / Decimal::from(total_quantity);
        let last_price = self.last_prices.get(symbol).copied().unwrap_or(avg_cost);
        let market_value = last_price * Decimal::from(total_quantity);
        let exchange = live_lots[0].exchange;

        Some(Position {
            symbol: symbol.to_owned(),
            exchange,
            total_quantity,
            t1_locked_quantity,
            available_quantity: total_quantity - t1_locked_quantity,
            avg_cost,
            last_price,
            market_value,
            unrealized_pnl: (last_price - avg_cost) * Decimal::from(total_quantity),
        })
    }

    fn consume_lots_fifo(
        &mut self,
        symbol: &str,
        mut quantity: i64,
        today: NaiveDate,
    ) -> Result<(), TgError> {
        let lots = self
            .lots
            .get_mut(symbol)
            .ok_or_else(|| TgError::RiskRejected("position does not exist".to_owned()))?;
        lots.sort_by_key(|lot| lot.trading_date);

        for lot in lots.iter_mut() {
            if quantity == 0 {
                break;
            }
            if lot.trading_date == today && !lot.t0_eligible {
                continue;
            }
            let take = lot.quantity.min(quantity);
            lot.quantity -= take;
            quantity -= take;
        }
        lots.retain(|lot| lot.quantity > 0);

        if quantity > 0 {
            return Err(TgError::RiskRejected(
                "sell quantity exceeds available lots".to_owned(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use tg_contracts::{Exchange, Fill, OrderSide};

    use super::{PositionLot, VirtualAccount};

    fn day(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
    }

    fn fill(symbol: &str, side: OrderSide, trading_date: NaiveDate, quantity: i64) -> Fill {
        Fill {
            order_id: "o".to_owned(),
            fill_id: "f".to_owned(),
            symbol: symbol.to_owned(),
            exchange: Exchange::Sh,
            side,
            price: Decimal::new(10, 0),
            quantity,
            commission: Decimal::ZERO,
            tax: Decimal::ZERO,
            transfer_fee: Decimal::ZERO,
            ts: chrono::Utc::now(),
            trading_date,
        }
    }

    #[test]
    fn t1_buy_today_is_locked_until_next_trading_day() {
        let mut account = VirtualAccount::new(Decimal::new(100_000, 0));
        account
            .apply_fill(&fill("600000", OrderSide::Buy, day(15), 200), false)
            .unwrap();
        let today = account.position("600000", day(15)).unwrap();
        assert_eq!(today.total_quantity, 200);
        assert_eq!(today.t1_locked_quantity, 200);
        assert_eq!(today.available_quantity, 0);

        let tomorrow = account.position("600000", day(16)).unwrap();
        assert_eq!(tomorrow.t1_locked_quantity, 0);
        assert_eq!(tomorrow.available_quantity, 200);
    }

    #[test]
    fn sell_consumes_available_lots_fifo() {
        let mut account = VirtualAccount::new(Decimal::new(100_000, 0));
        account.add_lot(PositionLot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            trading_date: day(14),
            quantity: 300,
            avg_cost: Decimal::new(8, 0),
            t0_eligible: false,
        });
        account
            .apply_fill(&fill("600000", OrderSide::Buy, day(15), 200), false)
            .unwrap();
        account.reserve_sell("600000", 200, day(15)).unwrap();
        account
            .apply_fill(&fill("600000", OrderSide::Sell, day(15), 200), false)
            .unwrap();
        let position = account.position("600000", day(15)).unwrap();
        assert_eq!(position.total_quantity, 300);
        assert_eq!(position.available_quantity, 100);
        assert_eq!(position.t1_locked_quantity, 200);
    }

    #[test]
    fn t0_eligible_etf_allows_same_day_sell_back() {
        let mut account = VirtualAccount::new(Decimal::new(100_000, 0));
        account
            .apply_fill(&fill("513000", OrderSide::Buy, day(15), 100), true)
            .unwrap();
        assert_eq!(
            account
                .position("513000", day(15))
                .unwrap()
                .available_quantity,
            100
        );
        account.reserve_sell("513000", 100, day(15)).unwrap();
        account
            .apply_fill(&fill("513000", OrderSide::Sell, day(15), 100), true)
            .unwrap();
        assert!(account.position("513000", day(15)).is_none());
    }
}
