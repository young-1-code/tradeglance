use std::collections::HashSet;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{
    OrderIntent, OrderSide, OrderType, Position, Snapshot, StrategyStyle, TimeInForce,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskConfig {
    pub single_name_concentration_cap: Decimal,
    pub total_exposure_cap: Decimal,
    pub blacklist: HashSet<String>,
    pub stop_loss_pct: Decimal,
    pub take_profit_pct: Decimal,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            single_name_concentration_cap: Decimal::new(20, 2),
            total_exposure_cap: Decimal::new(80, 2),
            blacklist: HashSet::new(),
            stop_loss_pct: Decimal::new(5, 2),
            take_profit_pct: Decimal::new(10, 2),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RiskEngine {
    pub config: RiskConfig,
}

impl RiskEngine {
    pub fn new(config: RiskConfig) -> Self {
        Self { config }
    }

    pub fn validate_soft(
        &self,
        intent: &OrderIntent,
        account_total_value: Decimal,
        current_position_value: Decimal,
        order_value: Decimal,
        total_position_value: Decimal,
    ) -> Result<(), String> {
        if self.config.blacklist.contains(&intent.symbol) {
            return Err("risk: blacklist".to_owned());
        }
        if matches!(intent.side, OrderSide::Buy) && account_total_value > Decimal::ZERO {
            let single_after = current_position_value + order_value;
            if single_after / account_total_value > self.config.single_name_concentration_cap {
                return Err("risk: single_position_cap".to_owned());
            }
            let total_after = total_position_value + order_value;
            if total_after / account_total_value > self.config.total_exposure_cap {
                return Err("risk: total_exposure_cap".to_owned());
            }
        }
        Ok(())
    }

    /// ADR-019 hard stop/take-profit path: this returns a direct SELL intent and
    /// is intentionally independent from any LLM or decision-agent path.
    pub fn hard_exit_intent(
        &self,
        position: &Position,
        snapshot: &Snapshot,
    ) -> Option<OrderIntent> {
        if position.available_quantity <= 0 || position.avg_cost <= Decimal::ZERO {
            return None;
        }
        let stop_price = position.avg_cost * (Decimal::ONE - self.config.stop_loss_pct);
        let take_price = position.avg_cost * (Decimal::ONE + self.config.take_profit_pct);
        let reason = if snapshot.last <= stop_price {
            "stoploss"
        } else if snapshot.last >= take_price {
            "takeprofit"
        } else {
            return None;
        };

        Some(OrderIntent {
            client_order_id: format!(
                "rule_{reason}_{}_{}",
                snapshot.symbol,
                snapshot.ts.timestamp_millis()
            ),
            symbol: snapshot.symbol.clone(),
            exchange: snapshot.exchange,
            side: OrderSide::Sell,
            order_type: OrderType::Market,
            price: None,
            quantity: position.available_quantity,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{Exchange, Position};

    use super::{RiskConfig, RiskEngine};

    #[test]
    fn hard_stop_loss_triggers_direct_sell_intent() {
        let risk = RiskEngine::new(RiskConfig::default());
        let snapshot = tg_contracts::Snapshot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            last: Decimal::new(94, 0),
            open: Decimal::new(100, 0),
            high: Decimal::new(100, 0),
            low: Decimal::new(94, 0),
            pre_close: Decimal::new(100, 0),
            volume: 1_000,
            amount: Decimal::new(94_000, 0),
            bid_price: [Decimal::new(94, 0); 5],
            bid_volume: [1_000; 5],
            ask_price: [Decimal::new(941, 1); 5],
            ask_volume: [1_000; 5],
        };
        let position = Position {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            total_quantity: 100,
            t1_locked_quantity: 0,
            available_quantity: 100,
            avg_cost: Decimal::new(100, 0),
            last_price: Decimal::new(94, 0),
            market_value: Decimal::new(9_400, 0),
            unrealized_pnl: Decimal::new(-600, 0),
        };
        let intent = risk.hard_exit_intent(&position, &snapshot).unwrap();
        assert_eq!(intent.quantity, 100);
        assert!(intent.client_order_id.contains("rule_stoploss"));
    }
}
