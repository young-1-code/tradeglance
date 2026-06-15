use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tg_contracts::{Account, Decision, FactorValue, IndicatorResult, Position, Signal, Snapshot};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextPayload {
    #[serde(default)]
    pub factors: Vec<FactorValue>,
    #[serde(default)]
    pub indicators: Vec<IndicatorResult>,
    #[serde(default)]
    pub market_snapshots: Vec<Snapshot>,
    #[serde(default)]
    pub positions: Vec<Position>,
    #[serde(default)]
    pub account: Option<Account>,
    #[serde(default)]
    pub recent_decisions: Vec<Decision>,
}

impl ContextPayload {
    pub fn from_json(raw: &str) -> anyhow::Result<Self> {
        if raw.trim().is_empty() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(raw)?)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionContext {
    pub signal: Signal,
    pub factors: Vec<FactorValue>,
    pub indicators: Vec<IndicatorResult>,
    pub market_snapshots: Vec<Snapshot>,
    pub positions: Vec<Position>,
    pub account: Option<Account>,
    pub recent_decisions: Vec<Decision>,
}

impl DecisionContext {
    pub fn new(signal: Signal, payload: ContextPayload) -> Self {
        Self {
            signal,
            factors: payload.factors,
            indicators: payload.indicators,
            market_snapshots: payload.market_snapshots,
            positions: payload.positions,
            account: payload.account,
            recent_decisions: payload.recent_decisions,
        }
    }

    pub fn symbol_position(&self) -> Option<&Position> {
        self.positions
            .iter()
            .find(|position| position.symbol == self.signal.symbol)
    }

    pub fn total_open_quantity(&self) -> i64 {
        self.positions
            .iter()
            .map(|position| position.total_quantity)
            .sum()
    }

    pub fn current_symbol_quantity(&self) -> i64 {
        self.symbol_position()
            .map(|position| position.total_quantity)
            .unwrap_or(0)
    }

    pub fn portfolio_market_value(&self) -> Decimal {
        self.positions
            .iter()
            .map(|position| position.market_value)
            .sum()
    }

    pub fn to_prompt_value(&self) -> Value {
        json!({
            "signal": self.signal,
            "factor_values": self.factors,
            "indicator_snapshot": self.indicators,
            "market_state": {
                "snapshots": self.market_snapshots,
            },
            "portfolio": {
                "positions": self.positions,
                "account": self.account,
                "portfolio_market_value": self.portfolio_market_value().to_string(),
            },
            "recent_decisions": self.recent_decisions.iter().map(|decision| {
                json!({
                    "id": decision.id,
                    "signal_id": decision.signal_id,
                    "symbol": decision.symbol,
                    "action": decision.action,
                    "side": decision.side,
                    "target_quantity": decision.target_quantity,
                    "rationale": decision.rationale,
                    "ts": decision.ts,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

pub fn build_user_prompt(context: &DecisionContext) -> anyhow::Result<String> {
    let prompt = json!({
        "task": "Decide whether this A-share short-term trading signal should become an executable decision.",
        "constraints": [
            "Open/Add/Reduce/Close/Hold only; hard stop-loss and take-profit are handled by mock-order-engine rules.",
            "Quantities must be non-negative and in 100-share lots.",
            "Use the supplied factor, indicator, market, portfolio, and recent-decision context."
        ],
        "context": context.to_prompt_value(),
    });
    Ok(serde_json::to_string_pretty(&prompt)?)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{
        Decision, DecisionAction, Exchange, FactorValue, IndicatorResult, OrderSide, Signal,
        SignalDirection, StrategyStyle,
    };

    use super::{build_user_prompt, ContextPayload, DecisionContext};

    fn signal() -> Signal {
        Signal {
            id: "sig-1".to_owned(),
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            direction: SignalDirection::Long,
            strength: 0.8,
            confidence: 0.7,
            style: StrategyStyle::Swing,
            reason: vec!["indicator:RSI(14)=28 < 30".to_owned()],
            suggested_quantity: Some(200),
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        }
    }

    #[test]
    fn assembled_prompt_contains_signal_factor_indicator_market_components() {
        let ts = Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap();
        let mut series = HashMap::new();
        series.insert("rsi".to_owned(), vec![28.0]);
        let context = DecisionContext::new(
            signal(),
            ContextPayload {
                factors: vec![FactorValue {
                    symbol: "600519".to_owned(),
                    factor: "momentum_20d".to_owned(),
                    ts,
                    trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
                    value: 1.2,
                    rank: Some(8),
                }],
                indicators: vec![IndicatorResult {
                    indicator: "RSI".to_owned(),
                    ts: vec![ts],
                    series,
                }],
                recent_decisions: vec![Decision {
                    id: "dec-1".to_owned(),
                    signal_id: Some("sig-0".to_owned()),
                    symbol: "600519".to_owned(),
                    exchange: Exchange::Sh,
                    action: DecisionAction::Hold,
                    side: OrderSide::Buy,
                    target_quantity: 0,
                    rationale: "previous hold".to_owned(),
                    risk_checks: vec![],
                    ts,
                }],
                ..ContextPayload::default()
            },
        );

        let prompt = build_user_prompt(&context).expect("render prompt");
        assert!(prompt.contains("600519"));
        assert!(prompt.contains("momentum_20d"));
        assert!(prompt.contains("RSI"));
        assert!(prompt.contains("market_state"));
        assert!(prompt.contains("recent_decisions"));
        assert!(prompt.contains(&Decimal::ZERO.to_string()) || prompt.contains("portfolio"));
    }
}
