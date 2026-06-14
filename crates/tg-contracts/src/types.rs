use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    BarPeriod, Board, DecisionAction, Exchange, InstrumentType, OrderSide, OrderStatus, OrderType,
    SignalDirection, StrategyStyle, TimeInForce,
};

pub type OrderId = String;
pub type SignalId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instrument {
    pub symbol: String,
    pub exchange: Exchange,
    pub instrument_type: InstrumentType,
    pub name: String,
    pub list_date: NaiveDate,
    pub delist_date: Option<NaiveDate>,
    pub is_st: bool,
    pub board: Board,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub symbol: String,
    pub exchange: Exchange,
    pub period: BarPeriod,
    pub ts: DateTime<Utc>,
    pub trading_date: NaiveDate,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub amount: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub symbol: String,
    pub exchange: Exchange,
    pub ts: DateTime<Utc>,
    pub trading_date: NaiveDate,
    pub last: Decimal,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub pre_close: Decimal,
    pub volume: i64,
    pub amount: Decimal,
    pub bid_price: [Decimal; 5],
    pub bid_volume: [i64; 5],
    pub ask_price: [Decimal; 5],
    pub ask_volume: [i64; 5],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdjustmentFactor {
    pub symbol: String,
    pub ex_date: NaiveDate,
    pub factor: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradingCalendar {
    pub date: NaiveDate,
    pub is_trading_day: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderIntent {
    pub client_order_id: String,
    pub symbol: String,
    pub exchange: Exchange,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub price: Option<Decimal>,
    pub quantity: i64,
    pub time_in_force: TimeInForce,
    pub strategy_tag: StrategyStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub client_order_id: String,
    pub symbol: String,
    pub exchange: Exchange,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub price: Option<Decimal>,
    pub quantity: i64,
    pub time_in_force: TimeInForce,
    pub strategy_tag: StrategyStyle,
    pub created_at: DateTime<Utc>,
    pub status: OrderStatus,
    pub filled_quantity: i64,
    pub avg_fill_price: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fill {
    pub order_id: OrderId,
    pub fill_id: String,
    pub symbol: String,
    pub exchange: Exchange,
    pub side: OrderSide,
    pub price: Decimal,
    pub quantity: i64,
    pub commission: Decimal,
    pub tax: Decimal,
    pub transfer_fee: Decimal,
    pub ts: DateTime<Utc>,
    pub trading_date: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub exchange: Exchange,
    pub total_quantity: i64,
    pub t1_locked_quantity: i64,
    pub available_quantity: i64,
    pub avg_cost: Decimal,
    pub last_price: Decimal,
    pub market_value: Decimal,
    pub unrealized_pnl: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub cash: Decimal,
    pub frozen_cash: Decimal,
    pub total_value: Decimal,
    pub positions: HashMap<String, Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndicatorRequest {
    pub indicator: String,
    pub params: HashMap<String, f64>,
    pub bars: Vec<Bar>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndicatorResult {
    pub indicator: String,
    pub ts: Vec<DateTime<Utc>>,
    pub series: HashMap<String, Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorValue {
    pub symbol: String,
    pub factor: String,
    pub ts: DateTime<Utc>,
    pub trading_date: NaiveDate,
    pub value: f64,
    pub rank: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorEvaluation {
    pub factor: String,
    pub ic_mean: f64,
    pub ic_std: f64,
    pub ir: f64,
    pub decay: Vec<f64>,
    pub quantile_returns: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub id: SignalId,
    pub symbol: String,
    pub exchange: Exchange,
    pub direction: SignalDirection,
    pub strength: f64,
    pub confidence: f64,
    pub style: StrategyStyle,
    pub reason: Vec<String>,
    pub suggested_quantity: Option<i64>,
    pub ts: DateTime<Utc>,
    pub trading_date: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskCheckResult {
    pub rule: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub signal_id: Option<SignalId>,
    pub symbol: String,
    pub exchange: Exchange,
    pub action: DecisionAction,
    pub side: OrderSide,
    pub target_quantity: i64,
    pub rationale: String,
    pub risk_checks: Vec<RiskCheckResult>,
    pub ts: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Bar(Bar),
    Snapshot(Snapshot),
    Timer(DateTime<Utc>),
    Fill(Fill),
}
