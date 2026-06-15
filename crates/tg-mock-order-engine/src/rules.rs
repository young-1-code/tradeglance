use std::collections::HashSet;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{
    limit_up_pct, Board, Exchange, Instrument, InstrumentType, OrderIntent, OrderSide, OrderType,
    Snapshot, StrategyStyle, TgError, LOT_SIZE,
};

use crate::account::VirtualAccount;
use crate::cost::{calculate_cost, CostConfig};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentRuleMeta {
    pub symbol: String,
    pub exchange: Exchange,
    pub instrument_type: InstrumentType,
    pub board: Board,
    pub is_st: bool,
    pub t0_eligible: bool,
}

impl InstrumentRuleMeta {
    pub fn stock(symbol: impl Into<String>, exchange: Exchange, board: Board) -> Self {
        Self {
            symbol: symbol.into(),
            exchange,
            instrument_type: InstrumentType::Stock,
            board,
            is_st: false,
            t0_eligible: false,
        }
    }

    pub fn from_instrument(instrument: &Instrument, t0_eligible: bool) -> Self {
        Self {
            symbol: instrument.symbol.clone(),
            exchange: instrument.exchange,
            instrument_type: instrument.instrument_type,
            board: instrument.board,
            is_st: instrument.is_st,
            t0_eligible,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuleEngine {
    pub cost: CostConfig,
    pub reject_st: bool,
    pub blacklist: HashSet<String>,
}

impl Default for RuleEngine {
    fn default() -> Self {
        Self {
            cost: CostConfig::default(),
            reject_st: true,
            blacklist: HashSet::new(),
        }
    }
}

impl RuleEngine {
    pub fn validate_submit(
        &self,
        intent: &OrderIntent,
        meta: &InstrumentRuleMeta,
        account: &VirtualAccount,
        latest: Option<&Snapshot>,
    ) -> Result<Decimal, TgError> {
        validate_quantity(intent.quantity)?;
        validate_symbol(intent, meta)?;
        validate_order_type(intent)?;
        if self.blacklist.contains(&intent.symbol) {
            return Err(TgError::RiskRejected("blacklisted symbol".to_owned()));
        }
        if self.reject_st && meta.is_st {
            return Err(TgError::RiskRejected("ST instrument rejected".to_owned()));
        }
        if let Some(snapshot) = latest {
            validate_price_band(intent, meta.board, snapshot.pre_close)?;
        }

        match intent.side {
            OrderSide::Buy => {
                let estimate_price = estimate_buy_price(intent, latest)?;
                let costs = calculate_cost(
                    OrderSide::Buy,
                    intent.exchange,
                    meta.instrument_type,
                    estimate_price,
                    intent.quantity,
                    self.cost,
                );
                let required = estimate_price * Decimal::from(intent.quantity) + costs.total();
                if account.available_cash() < required {
                    return Err(TgError::RiskRejected("insufficient cash".to_owned()));
                }
                Ok(required)
            }
            OrderSide::Sell => {
                let trading_date =
                    latest
                        .map(|snapshot| snapshot.trading_date)
                        .ok_or_else(|| {
                            TgError::InvalidOrder(
                                "sell validation requires latest snapshot".to_owned(),
                            )
                        })?;
                if account.available_to_reserve(&intent.symbol, trading_date) < intent.quantity {
                    return Err(TgError::RiskRejected(
                        "insufficient available position".to_owned(),
                    ));
                }
                Ok(Decimal::ZERO)
            }
        }
    }
}

pub fn limit_prices(pre_close: Decimal, board: Board) -> (Decimal, Decimal) {
    let pct = limit_up_pct(board);
    (
        (pre_close * (Decimal::ONE + pct)).round_dp(2),
        (pre_close * (Decimal::ONE - pct)).round_dp(2),
    )
}

fn validate_quantity(quantity: i64) -> Result<(), TgError> {
    if quantity <= 0 || quantity % LOT_SIZE != 0 {
        return Err(TgError::InvalidOrder(format!(
            "quantity must be a positive multiple of {LOT_SIZE}"
        )));
    }
    Ok(())
}

fn validate_symbol(intent: &OrderIntent, meta: &InstrumentRuleMeta) -> Result<(), TgError> {
    if intent.symbol != meta.symbol || intent.exchange != meta.exchange {
        return Err(TgError::InvalidOrder(
            "instrument metadata mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn validate_order_type(intent: &OrderIntent) -> Result<(), TgError> {
    if matches!(intent.order_type, OrderType::Limit) && intent.price.is_none() {
        return Err(TgError::InvalidOrder(
            "limit order requires price".to_owned(),
        ));
    }
    Ok(())
}

fn validate_price_band(
    intent: &OrderIntent,
    board: Board,
    pre_close: Decimal,
) -> Result<(), TgError> {
    let Some(price) = intent.price else {
        return Ok(());
    };
    let (limit_up, limit_down) = limit_prices(pre_close, board);
    if price < limit_down || price > limit_up {
        return Err(TgError::InvalidOrder(format!(
            "price {price} outside daily band [{limit_down}, {limit_up}]"
        )));
    }
    Ok(())
}

fn estimate_buy_price(intent: &OrderIntent, latest: Option<&Snapshot>) -> Result<Decimal, TgError> {
    match (intent.order_type, intent.price, latest) {
        (OrderType::Limit, Some(price), _) => Ok(price),
        (OrderType::Market, _, Some(snapshot)) if snapshot.ask_price[0] > Decimal::ZERO => {
            Ok(snapshot.ask_price[0])
        }
        (OrderType::Market, _, _) => Err(TgError::InvalidOrder(
            "market buy requires latest ask quote".to_owned(),
        )),
        (OrderType::Limit, None, _) => Err(TgError::InvalidOrder(
            "limit order requires price".to_owned(),
        )),
    }
}

pub fn is_t0_candidate(meta: &InstrumentRuleMeta, strategy: StrategyStyle) -> bool {
    matches!(meta.instrument_type, InstrumentType::Etf)
        && meta.t0_eligible
        && matches!(strategy, StrategyStyle::T0)
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{
        Board, Exchange, OrderIntent, OrderSide, OrderType, StrategyStyle, TimeInForce,
    };

    use crate::account::{PositionLot, VirtualAccount};

    use super::{InstrumentRuleMeta, RuleEngine};

    fn snapshot() -> tg_contracts::Snapshot {
        tg_contracts::Snapshot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            last: Decimal::new(10, 0),
            open: Decimal::new(10, 0),
            high: Decimal::new(10, 0),
            low: Decimal::new(10, 0),
            pre_close: Decimal::new(10, 0),
            volume: 1_000,
            amount: Decimal::new(10_000, 0),
            bid_price: [Decimal::new(999, 2); 5],
            bid_volume: [1_000; 5],
            ask_price: [Decimal::new(1001, 2); 5],
            ask_volume: [1_000; 5],
        }
    }

    fn intent(price: Decimal, quantity: i64, side: OrderSide) -> OrderIntent {
        OrderIntent {
            client_order_id: "c".to_owned(),
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            side,
            order_type: OrderType::Limit,
            price: Some(price),
            quantity,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
        }
    }

    #[test]
    fn rejects_over_daily_limit_and_non_lot_quantity() {
        let account = VirtualAccount::new(Decimal::new(100_000, 0));
        let rules = RuleEngine::default();
        let meta = InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard);
        assert!(rules
            .validate_submit(
                &intent(Decimal::new(111, 1), 100, OrderSide::Buy),
                &meta,
                &account,
                Some(&snapshot())
            )
            .is_err());
        assert!(rules
            .validate_submit(
                &intent(Decimal::new(10, 0), 150, OrderSide::Buy),
                &meta,
                &account,
                Some(&snapshot())
            )
            .is_err());
    }

    #[test]
    fn rejects_insufficient_cash_and_available_position() {
        let rules = RuleEngine::default();
        let meta = InstrumentRuleMeta::stock("600000", Exchange::Sh, Board::MainBoard);
        let account = VirtualAccount::new(Decimal::new(1, 0));
        assert!(rules
            .validate_submit(
                &intent(Decimal::new(10, 0), 100, OrderSide::Buy),
                &meta,
                &account,
                Some(&snapshot())
            )
            .is_err());

        let mut account = VirtualAccount::new(Decimal::new(100_000, 0));
        account.add_lot(PositionLot {
            symbol: "600000".to_owned(),
            exchange: Exchange::Sh,
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            quantity: 100,
            avg_cost: Decimal::new(10, 0),
            t0_eligible: false,
        });
        assert!(rules
            .validate_submit(
                &intent(Decimal::new(10, 0), 100, OrderSide::Sell),
                &meta,
                &account,
                Some(&snapshot())
            )
            .is_err());
    }
}
