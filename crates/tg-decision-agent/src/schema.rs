use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tg_contracts::{DecisionAction, OrderSide, LOT_SIZE};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDecision {
    pub action: DecisionAction,
    pub side: OrderSide,
    pub target_quantity: i64,
    pub rationale: String,
    pub risk_notes: String,
}

pub fn decision_json_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action", "side", "target_quantity", "rationale", "risk_notes"],
        "properties": {
            "action": {"type": "string", "enum": ["open", "add", "reduce", "close", "hold", "Open", "Add", "Reduce", "Close", "Hold"]},
            "side": {"type": "string", "enum": ["buy", "sell", "Buy", "Sell"]},
            "target_quantity": {"type": "integer", "minimum": 0, "multipleOf": 100},
            "rationale": {"type": "string", "maxLength": 1000},
            "risk_notes": {"type": "string", "maxLength": 1000}
        }
    })
}

pub fn parse_decision(raw_json: &str) -> Result<ParsedDecision> {
    let value: Value = serde_json::from_str(raw_json)?;
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("decision output must be a JSON object"))?;

    let action = parse_action(required_str(object, "action")?)?;
    let side = parse_side(required_str(object, "side")?)?;
    let target_quantity = object
        .get("target_quantity")
        .and_then(Value::as_i64)
        .ok_or_else(|| anyhow!("target_quantity must be an integer"))?;
    validate_quantity(target_quantity)?;

    let rationale = required_str(object, "rationale")?.trim().to_owned();
    if rationale.is_empty() {
        return Err(anyhow!("rationale must not be empty"));
    }
    if rationale.chars().count() > 1000 {
        return Err(anyhow!("rationale exceeds 1000 characters"));
    }

    let risk_notes = object
        .get("risk_notes")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_owned();
    if risk_notes.chars().count() > 1000 {
        return Err(anyhow!("risk_notes exceeds 1000 characters"));
    }

    Ok(ParsedDecision {
        action,
        side,
        target_quantity,
        rationale,
        risk_notes,
    })
}

fn required_str<'a>(object: &'a serde_json::Map<String, Value>, field: &str) -> Result<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("{field} must be a string"))
}

fn parse_action(value: &str) -> Result<DecisionAction> {
    match value {
        "open" | "Open" => Ok(DecisionAction::Open),
        "add" | "Add" => Ok(DecisionAction::Add),
        "reduce" | "Reduce" => Ok(DecisionAction::Reduce),
        "close" | "Close" => Ok(DecisionAction::Close),
        "hold" | "Hold" => Ok(DecisionAction::Hold),
        _ => Err(anyhow!("unsupported action: {value}")),
    }
}

fn parse_side(value: &str) -> Result<OrderSide> {
    match value {
        "buy" | "Buy" => Ok(OrderSide::Buy),
        "sell" | "Sell" => Ok(OrderSide::Sell),
        _ => Err(anyhow!("unsupported side: {value}")),
    }
}

fn validate_quantity(quantity: i64) -> Result<()> {
    if quantity < 0 {
        return Err(anyhow!("target_quantity must be non-negative"));
    }
    if quantity % LOT_SIZE != 0 {
        return Err(anyhow!("target_quantity must be a multiple of {LOT_SIZE}"));
    }
    Ok(())
}

pub(crate) fn clamp_to_lot(quantity: i64) -> i64 {
    if quantity <= 0 {
        0
    } else {
        quantity / LOT_SIZE * LOT_SIZE
    }
}

#[cfg(test)]
mod tests {
    use tg_contracts::{DecisionAction, OrderSide};

    use super::parse_decision;

    #[test]
    fn valid_json_parses_to_decision() {
        let parsed = parse_decision(
            r#"{"action":"open","side":"buy","target_quantity":200,"rationale":"strong setup","risk_notes":"ok"}"#,
        )
        .expect("valid decision");
        assert_eq!(parsed.action, DecisionAction::Open);
        assert_eq!(parsed.side, OrderSide::Buy);
        assert_eq!(parsed.target_quantity, 200);
    }

    #[test]
    fn invalid_action_is_rejected() {
        assert!(parse_decision(
            r#"{"action":"wait","side":"buy","target_quantity":200,"rationale":"x","risk_notes":""}"#
        )
        .is_err());
    }

    #[test]
    fn quantity_must_be_lot_sized_and_non_negative() {
        assert!(parse_decision(
            r#"{"action":"open","side":"buy","target_quantity":150,"rationale":"x","risk_notes":""}"#
        )
        .is_err());
        assert!(parse_decision(
            r#"{"action":"open","side":"buy","target_quantity":-100,"rationale":"x","risk_notes":""}"#
        )
        .is_err());
    }
}
