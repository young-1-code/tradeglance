use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{
    Exchange, InstrumentType, OrderSide, COMMISSION_MAX_PCT, STAMP_DUTY_PCT, TRANSFER_FEE_PCT,
};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CostConfig {
    pub commission_rate: Decimal,
    pub min_commission: Decimal,
}

impl Default for CostConfig {
    fn default() -> Self {
        Self {
            commission_rate: COMMISSION_MAX_PCT,
            min_commission: Decimal::new(5, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBreakdown {
    pub commission: Decimal,
    pub tax: Decimal,
    pub transfer_fee: Decimal,
}

impl CostBreakdown {
    pub fn total(self) -> Decimal {
        self.commission + self.tax + self.transfer_fee
    }
}

pub fn calculate_cost(
    side: OrderSide,
    exchange: Exchange,
    instrument_type: InstrumentType,
    price: Decimal,
    quantity: i64,
    config: CostConfig,
) -> CostBreakdown {
    let notional = price * Decimal::from(quantity);
    let commission = (notional * config.commission_rate)
        .max(config.min_commission)
        .round_dp(4);
    let tax = if matches!(side, OrderSide::Sell) && !matches!(instrument_type, InstrumentType::Etf)
    {
        (notional * STAMP_DUTY_PCT).round_dp(4)
    } else {
        Decimal::ZERO
    };
    let transfer_fee = if matches!(exchange, Exchange::Sh) {
        (notional * TRANSFER_FEE_PCT).round_dp(4)
    } else {
        Decimal::ZERO
    };

    CostBreakdown {
        commission,
        tax,
        transfer_fee,
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use tg_contracts::{Exchange, InstrumentType, OrderSide};

    use super::{calculate_cost, CostConfig};

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    #[test]
    fn stock_sell_charges_commission_stamp_and_sh_transfer_fee() {
        let cost = calculate_cost(
            OrderSide::Sell,
            Exchange::Sh,
            InstrumentType::Stock,
            dec(10, 0),
            10_000,
            CostConfig::default(),
        );
        assert_eq!(cost.commission, dec(30, 0));
        assert_eq!(cost.tax, dec(50, 0));
        assert_eq!(cost.transfer_fee, dec(1, 0));
    }

    #[test]
    fn etf_sell_has_no_stamp_duty() {
        let cost = calculate_cost(
            OrderSide::Sell,
            Exchange::Sh,
            InstrumentType::Etf,
            dec(10, 0),
            10_000,
            CostConfig::default(),
        );
        assert_eq!(cost.commission, dec(30, 0));
        assert_eq!(cost.tax, Decimal::ZERO);
        assert_eq!(cost.transfer_fee, dec(1, 0));
    }

    #[test]
    fn sz_buy_has_commission_but_no_transfer_fee_or_tax() {
        let cost = calculate_cost(
            OrderSide::Buy,
            Exchange::Sz,
            InstrumentType::Stock,
            dec(10, 0),
            1_000,
            CostConfig::default(),
        );
        assert_eq!(cost.commission, dec(5, 0));
        assert_eq!(cost.tax, Decimal::ZERO);
        assert_eq!(cost.transfer_fee, Decimal::ZERO);
    }
}
