use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tg_contracts::{FactorValue, SignalDirection, StrategyStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

impl CmpOp {
    pub fn compare_f64(self, left: f64, right: f64) -> bool {
        match self {
            Self::Lt => left < right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Ge => left >= right,
            Self::Eq => (left - right).abs() <= f64::EPSILON,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Eq => "==",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceField {
    Open,
    High,
    Low,
    Close,
    Last,
    PreClose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Condition {
    IndicatorThreshold {
        key: String,
        op: CmpOp,
        threshold: f64,
        reason_code: String,
    },
    FactorRankThreshold {
        factor: String,
        max_rank_pct: f64,
        universe: u32,
        reason_code: String,
    },
    PriceVsLevel {
        field: PriceField,
        op: CmpOp,
        level: Decimal,
        reason_code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConditionTree {
    Leaf(Condition),
    And(Vec<ConditionTree>),
    Or(Vec<ConditionTree>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub style: StrategyStyle,
    pub direction: SignalDirection,
    pub condition: ConditionTree,
}

#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    pub indicator_values: HashMap<String, f64>,
    pub factor_values: HashMap<String, FactorValue>,
    pub price_values: HashMap<PriceField, Decimal>,
}

impl EvalContext {
    pub fn with_indicator(mut self, key: impl Into<String>, value: f64) -> Self {
        self.indicator_values.insert(key.into(), value);
        self
    }

    pub fn with_factor(mut self, value: FactorValue) -> Self {
        self.factor_values.insert(value.factor.clone(), value);
        self
    }

    pub fn with_price(mut self, field: PriceField, value: Decimal) -> Self {
        self.price_values.insert(field, value);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuleEvaluation {
    pub fired: bool,
    pub reasons: Vec<String>,
    pub strength: f64,
    pub confidence: f64,
}

impl Rule {
    pub fn evaluate(&self, ctx: &EvalContext) -> RuleEvaluation {
        self.condition.evaluate(ctx)
    }
}

impl ConditionTree {
    pub fn evaluate(&self, ctx: &EvalContext) -> RuleEvaluation {
        match self {
            Self::Leaf(condition) => condition.evaluate(ctx),
            Self::And(children) => evaluate_and(children, ctx),
            Self::Or(children) => evaluate_or(children, ctx),
        }
    }
}

impl Condition {
    pub fn evaluate(&self, ctx: &EvalContext) -> RuleEvaluation {
        match self {
            Self::IndicatorThreshold {
                key,
                op,
                threshold,
                reason_code,
            } => match ctx.indicator_values.get(key).copied() {
                Some(value) if op.compare_f64(value, *threshold) => fired(format!(
                    "indicator:{reason_code} {key}={value:.4} {} {threshold:.4}",
                    op.as_str()
                )),
                _ => not_fired(),
            },
            Self::FactorRankThreshold {
                factor,
                max_rank_pct,
                universe,
                reason_code,
            } => match ctx.factor_values.get(factor).and_then(|value| value.rank) {
                Some(rank) if *universe > 0 => {
                    let rank_pct = f64::from(rank) / f64::from(*universe);
                    if rank_pct <= *max_rank_pct {
                        fired(format!(
                            "factor:{reason_code} {factor} rank top {:.2}% (rank={rank}/{universe})",
                            rank_pct * 100.0
                        ))
                    } else {
                        not_fired()
                    }
                }
                _ => not_fired(),
            },
            Self::PriceVsLevel {
                field,
                op,
                level,
                reason_code,
            } => match ctx
                .price_values
                .get(field)
                .and_then(|value| value.to_f64())
                .zip(level.to_f64())
            {
                Some((left, right)) if op.compare_f64(left, right) => fired(format!(
                    "price:{reason_code} {field:?}={left:.4} {} {right:.4}",
                    op.as_str()
                )),
                _ => not_fired(),
            },
        }
    }
}

fn evaluate_and(children: &[ConditionTree], ctx: &EvalContext) -> RuleEvaluation {
    if children.is_empty() {
        return not_fired();
    }

    let mut reasons = Vec::new();
    let mut fired_count = 0usize;
    for child in children {
        let result = child.evaluate(ctx);
        if result.fired {
            fired_count += 1;
            reasons.extend(result.reasons);
        }
    }

    let fired = fired_count == children.len();
    let ratio = fired_count as f64 / children.len() as f64;
    RuleEvaluation {
        fired,
        reasons: if fired { reasons } else { Vec::new() },
        strength: ratio,
        confidence: ratio,
    }
}

fn evaluate_or(children: &[ConditionTree], ctx: &EvalContext) -> RuleEvaluation {
    if children.is_empty() {
        return not_fired();
    }

    let mut reasons = Vec::new();
    let mut fired_count = 0usize;
    for child in children {
        let result = child.evaluate(ctx);
        if result.fired {
            fired_count += 1;
            reasons.extend(result.reasons);
        }
    }

    let fired = fired_count > 0;
    let ratio = fired_count as f64 / children.len() as f64;
    RuleEvaluation {
        fired,
        reasons,
        strength: if fired { ratio.max(0.5) } else { 0.0 },
        confidence: ratio,
    }
}

fn fired(reason: String) -> RuleEvaluation {
    RuleEvaluation {
        fired: true,
        reasons: vec![reason],
        strength: 1.0,
        confidence: 1.0,
    }
}

fn not_fired() -> RuleEvaluation {
    RuleEvaluation {
        fired: false,
        reasons: Vec::new(),
        strength: 0.0,
        confidence: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::FactorValue;

    use super::*;

    #[test]
    fn and_combination_fires_and_collects_reason_codes() {
        let rule = ConditionTree::And(vec![
            ConditionTree::Leaf(Condition::IndicatorThreshold {
                key: "RSI14".to_owned(),
                op: CmpOp::Lt,
                threshold: 50.0,
                reason_code: "RSI(14)".to_owned(),
            }),
            ConditionTree::Leaf(Condition::PriceVsLevel {
                field: PriceField::Close,
                op: CmpOp::Gt,
                level: Decimal::new(1000, 2),
                reason_code: "close_above_level".to_owned(),
            }),
        ]);
        let ctx = EvalContext::default()
            .with_indicator("RSI14", 42.0)
            .with_price(PriceField::Close, Decimal::new(1050, 2));

        let result = rule.evaluate(&ctx);

        assert!(result.fired);
        assert_eq!(result.reasons.len(), 2);
        assert!(result.reasons[0].contains("indicator:RSI(14)"));
        assert!(result.reasons[1].contains("price:close_above_level"));
    }

    #[test]
    fn or_combination_fires_when_any_child_matches() {
        let rule = ConditionTree::Or(vec![
            ConditionTree::Leaf(Condition::IndicatorThreshold {
                key: "RSI14".to_owned(),
                op: CmpOp::Gt,
                threshold: 70.0,
                reason_code: "RSI(14)".to_owned(),
            }),
            ConditionTree::Leaf(Condition::PriceVsLevel {
                field: PriceField::Close,
                op: CmpOp::Gt,
                level: Decimal::new(1000, 2),
                reason_code: "breakout".to_owned(),
            }),
        ]);
        let ctx = EvalContext::default()
            .with_indicator("RSI14", 45.0)
            .with_price(PriceField::Close, Decimal::new(1100, 2));

        let result = rule.evaluate(&ctx);

        assert!(result.fired);
        assert_eq!(result.reasons.len(), 1);
        assert!(result.reasons[0].contains("price:breakout"));
    }

    #[test]
    fn factor_rank_threshold_uses_rank_percentile() {
        let value = FactorValue {
            symbol: "600000".to_owned(),
            factor: "momentum_20d".to_owned(),
            ts: Utc::now(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).expect("valid date"),
            value: 1.2,
            rank: Some(3),
        };
        let condition = Condition::FactorRankThreshold {
            factor: "momentum_20d".to_owned(),
            max_rank_pct: 0.2,
            universe: 20,
            reason_code: "momentum_20d".to_owned(),
        };

        let result = condition.evaluate(&EvalContext::default().with_factor(value));

        assert!(result.fired);
        assert!(result.reasons[0].contains("factor:momentum_20d"));
    }
}
