#![forbid(unsafe_code)]

pub mod enums;
pub mod error;
pub mod proto;
pub mod query;
pub mod rules;
pub mod time;
pub mod types;

pub use enums::*;
pub use error::{Result, TgError};
pub use query::BarQuery;
pub use rules::{limit_up_pct, COMMISSION_MAX_PCT, LOT_SIZE, STAMP_DUTY_PCT, TRANSFER_FEE_PCT};
pub use time::{is_call_auction, is_continuous_auction};
pub use types::*;

#[cfg(test)]
mod tests {
    use chrono::{DateTime, NaiveDate, Utc};
    use rust_decimal::Decimal;

    use super::*;

    fn ts() -> DateTime<Utc> {
        "2026-06-15T02:00:00Z".parse().expect("valid UTC timestamp")
    }

    fn trading_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, 15).expect("valid trading date")
    }

    fn dec(value: i64, scale: u32) -> Decimal {
        Decimal::new(value, scale)
    }

    fn sample_bar() -> Bar {
        Bar {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            period: BarPeriod::Daily,
            ts: ts(),
            trading_date: trading_date(),
            open: dec(150_000, 2),
            high: dec(151_000, 2),
            low: dec(149_000, 2),
            close: dec(150_500, 2),
            volume: 10_000,
            amount: dec(1_505_000_000, 2),
        }
    }

    fn sample_order() -> Order {
        Order {
            id: "01JZ0000000000000000000000".to_owned(),
            client_order_id: "client-1".to_owned(),
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            price: Some(dec(150_000, 2)),
            quantity: 100,
            time_in_force: TimeInForce::Day,
            strategy_tag: StrategyStyle::Swing,
            created_at: ts(),
            status: OrderStatus::New,
            filled_quantity: 0,
            avg_fill_price: dec(0, 0),
        }
    }

    fn sample_snapshot() -> Snapshot {
        Snapshot {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            ts: ts(),
            trading_date: trading_date(),
            last: dec(150_500, 2),
            open: dec(150_000, 2),
            high: dec(151_000, 2),
            low: dec(149_000, 2),
            pre_close: dec(149_800, 2),
            volume: 10_000,
            amount: dec(1_505_000_000, 2),
            bid_price: [
                dec(150_490, 2),
                dec(150_480, 2),
                dec(150_470, 2),
                dec(150_460, 2),
                dec(150_450, 2),
            ],
            bid_volume: [100, 200, 300, 400, 500],
            ask_price: [
                dec(150_510, 2),
                dec(150_520, 2),
                dec(150_530, 2),
                dec(150_540, 2),
                dec(150_550, 2),
            ],
            ask_volume: [100, 200, 300, 400, 500],
        }
    }

    fn sample_signal() -> Signal {
        Signal {
            id: "01JZ0000000000000000000001".to_owned(),
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            direction: SignalDirection::Long,
            strength: 0.8,
            confidence: 0.7,
            style: StrategyStyle::Swing,
            reason: vec!["RSI_OVERSOLD".to_owned()],
            suggested_quantity: Some(100),
            ts: ts(),
            trading_date: trading_date(),
        }
    }

    #[test]
    fn serde_json_round_trips_domain_types() {
        let bar_json = serde_json::to_string(&sample_bar()).expect("serialize bar");
        assert_eq!(
            serde_json::from_str::<Bar>(&bar_json).expect("deserialize bar"),
            sample_bar()
        );

        let order_json = serde_json::to_string(&sample_order()).expect("serialize order");
        assert_eq!(
            serde_json::from_str::<Order>(&order_json).expect("deserialize order"),
            sample_order()
        );

        let snapshot_json = serde_json::to_string(&sample_snapshot()).expect("serialize snapshot");
        assert_eq!(
            serde_json::from_str::<Snapshot>(&snapshot_json).expect("deserialize snapshot"),
            sample_snapshot()
        );

        let signal_json = serde_json::to_string(&sample_signal()).expect("serialize signal");
        assert_eq!(
            serde_json::from_str::<Signal>(&signal_json).expect("deserialize signal"),
            sample_signal()
        );
    }

    #[test]
    fn fill_cost_uses_decimal_arithmetic() {
        let fill = Fill {
            order_id: "01JZ0000000000000000000000".to_owned(),
            fill_id: "fill-1".to_owned(),
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            side: OrderSide::Sell,
            price: dec(1025, 2),
            quantity: 200,
            commission: dec(615, 3),
            tax: dec(1025, 3),
            transfer_fee: dec(205, 4),
            ts: ts(),
            trading_date: trading_date(),
        };

        let gross = fill.price * Decimal::from(fill.quantity);
        let net = gross - fill.commission - fill.tax - fill.transfer_fee;
        assert_eq!(gross, dec(205_000, 2));
        assert_eq!(net, dec(20_483_395, 4));
    }
}
