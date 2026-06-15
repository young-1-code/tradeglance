use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{is_call_auction, Fill, Order, OrderSide, OrderStatus, OrderType, Snapshot};

use crate::cost::{calculate_cost, CostConfig};
use crate::rules::{limit_prices, InstrumentRuleMeta};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MatchConfig {
    pub fill_ratio: Decimal,
    pub slippage_bps: i64,
    pub seal_multiplier: i64,
}

impl Default for MatchConfig {
    fn default() -> Self {
        Self {
            fill_ratio: Decimal::new(3, 1),
            slippage_bps: 2,
            seal_multiplier: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchEngine {
    pub config: MatchConfig,
    pub cost: CostConfig,
}

impl MatchEngine {
    pub fn new(config: MatchConfig, cost: CostConfig) -> Self {
        Self { config, cost }
    }

    pub fn try_match(
        &self,
        order: &Order,
        snapshot: &Snapshot,
        meta: &InstrumentRuleMeta,
    ) -> Option<Fill> {
        if is_call_auction(snapshot.ts) || order.symbol != snapshot.symbol || is_terminal(order) {
            return None;
        }
        let remaining = order.quantity - order.filled_quantity;
        if remaining <= 0 {
            return None;
        }
        if !self.limit_gate(order, snapshot, meta) {
            return None;
        }

        let (book_price, book_volume) = match order.side {
            OrderSide::Buy => (snapshot.ask_price[0], snapshot.ask_volume[0]),
            OrderSide::Sell => (snapshot.bid_price[0], snapshot.bid_volume[0]),
        };
        if book_price <= Decimal::ZERO || book_volume <= 0 {
            return None;
        }
        let ratio_qty = (Decimal::from(book_volume) * self.config.fill_ratio)
            .floor()
            .to_i64()
            .unwrap_or(0);
        let quantity = remaining.min(ratio_qty);
        if quantity <= 0 {
            return None;
        }

        let price = self.apply_slippage(book_price, order.side).round_dp(4);
        let cost = calculate_cost(
            order.side,
            order.exchange,
            meta.instrument_type,
            price,
            quantity,
            self.cost,
        );
        Some(Fill {
            order_id: order.id.clone(),
            fill_id: crate::handler::new_id("fill"),
            symbol: order.symbol.clone(),
            exchange: order.exchange,
            side: order.side,
            price,
            quantity,
            commission: cost.commission,
            tax: cost.tax,
            transfer_fee: cost.transfer_fee,
            ts: snapshot.ts,
            trading_date: snapshot.trading_date,
        })
    }

    fn limit_gate(&self, order: &Order, snapshot: &Snapshot, meta: &InstrumentRuleMeta) -> bool {
        if self.at_limit_gate_blocked(order.side, snapshot, meta) {
            return false;
        }
        match (order.order_type, order.side, order.price) {
            (OrderType::Market, _, _) => true,
            (OrderType::Limit, OrderSide::Buy, Some(price)) => snapshot.ask_price[0] <= price,
            (OrderType::Limit, OrderSide::Sell, Some(price)) => snapshot.bid_price[0] >= price,
            (OrderType::Limit, _, None) => false,
        }
    }

    fn at_limit_gate_blocked(
        &self,
        side: OrderSide,
        snapshot: &Snapshot,
        meta: &InstrumentRuleMeta,
    ) -> bool {
        let (limit_up, limit_down) = limit_prices(snapshot.pre_close, meta.board);
        match side {
            OrderSide::Buy if snapshot.last == limit_up => {
                snapshot.bid_volume[0] <= self.config.seal_multiplier * snapshot.ask_volume[0]
            }
            OrderSide::Sell if snapshot.last == limit_down => {
                snapshot.ask_volume[0] <= self.config.seal_multiplier * snapshot.bid_volume[0]
            }
            _ => false,
        }
    }

    fn apply_slippage(&self, price: Decimal, side: OrderSide) -> Decimal {
        let bps = Decimal::from(self.config.slippage_bps) / Decimal::from(10_000);
        match side {
            OrderSide::Buy => price * (Decimal::ONE + bps),
            OrderSide::Sell => price * (Decimal::ONE - bps),
        }
    }
}

fn is_terminal(order: &Order) -> bool {
    matches!(
        order.status,
        OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
    )
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{
        Board, Exchange, Order, OrderSide, OrderStatus, OrderType, StrategyStyle, TimeInForce,
    };

    use super::{MatchConfig, MatchEngine};
    use crate::cost::CostConfig;
    use crate::rules::InstrumentRuleMeta;

    fn snapshot(last: Decimal) -> tg_contracts::Snapshot {
        tg_contracts::Snapshot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            last,
            open: Decimal::new(10, 0),
            high: Decimal::new(10, 0),
            low: Decimal::new(10, 0),
            pre_close: Decimal::new(10, 0),
            volume: 10_000,
            amount: Decimal::new(100_000, 0),
            bid_price: [Decimal::new(999, 2); 5],
            bid_volume: [1_000; 5],
            ask_price: [Decimal::new(1001, 2); 5],
            ask_volume: [1_000; 5],
        }
    }

    fn order(side: OrderSide, order_type: OrderType, price: Option<Decimal>) -> Order {
        Order {
            id: "o".to_owned(),
            client_order_id: "c".to_owned(),
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            side,
            order_type,
            price,
            quantity: 1_000,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
            created_at: Utc::now(),
            status: OrderStatus::New,
            filled_quantity: 0,
            avg_fill_price: Decimal::ZERO,
        }
    }

    #[test]
    fn market_buy_partial_fills_with_slippage() {
        let matcher = MatchEngine::new(MatchConfig::default(), CostConfig::default());
        let fill = matcher
            .try_match(
                &order(OrderSide::Buy, OrderType::Market, None),
                &snapshot(Decimal::new(10, 0)),
                &InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard),
            )
            .unwrap();
        assert_eq!(fill.quantity, 300);
        assert_eq!(fill.price, Decimal::new(10012002, 6).round_dp(4));
    }

    #[test]
    fn limit_order_fills_inside_spread_and_waits_outside() {
        let matcher = MatchEngine::new(MatchConfig::default(), CostConfig::default());
        assert!(matcher
            .try_match(
                &order(
                    OrderSide::Buy,
                    OrderType::Limit,
                    Some(Decimal::new(1010, 2))
                ),
                &snapshot(Decimal::new(10, 0)),
                &InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard),
            )
            .is_some());
        assert!(matcher
            .try_match(
                &order(
                    OrderSide::Buy,
                    OrderType::Limit,
                    Some(Decimal::new(1000, 2))
                ),
                &snapshot(Decimal::new(10, 0)),
                &InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard),
            )
            .is_none());
    }

    #[test]
    fn limit_up_buy_requires_seal_gate() {
        let matcher = MatchEngine::new(MatchConfig::default(), CostConfig::default());
        let mut snap = snapshot(Decimal::new(11, 0));
        snap.ask_price[0] = Decimal::new(11, 0);
        let buy = order(OrderSide::Buy, OrderType::Limit, Some(Decimal::new(11, 0)));
        let meta = InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard);
        assert!(matcher.try_match(&buy, &snap, &meta).is_none());
        snap.bid_volume[0] = 6_000;
        assert!(matcher.try_match(&buy, &snap, &meta).is_some());
    }
}
