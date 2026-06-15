use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tg_contracts::{Decision, DecisionAction, OrderSide, RiskCheckResult};

use crate::context::{build_user_prompt, DecisionContext};
use crate::fallback::LlmAvailability;
use crate::llm::LlmClient;
use crate::logging::{DecisionLogger, NoopDecisionLogger};
use crate::schema::{clamp_to_lot, decision_json_schema, parse_decision, ParsedDecision};

static DECISION_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskConfig {
    pub max_single_name_quantity: i64,
    pub max_total_quantity: i64,
    pub blacklist: Vec<String>,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_single_name_quantity: 20_000,
            max_total_quantity: 100_000,
            blacklist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OrchestratorConfig {
    pub risk: RiskConfig,
}

pub struct AnalystAgent<C> {
    llm: Arc<C>,
}

impl<C> AnalystAgent<C>
where
    C: LlmClient,
{
    pub fn new(llm: Arc<C>) -> Self {
        Self { llm }
    }

    pub async fn analyze(&self, context: &DecisionContext) -> Result<String> {
        let prompt = build_user_prompt(context)?;
        self.llm
            .chat(
                "You are the analyst agent. Interpret the signal, factors, indicators, market state, and recent decisions. Do not decide action or quantity.",
                &prompt,
                None,
            )
            .await
    }
}

pub struct TraderAgent<C> {
    llm: Arc<C>,
}

impl<C> TraderAgent<C>
where
    C: LlmClient,
{
    pub fn new(llm: Arc<C>) -> Self {
        Self { llm }
    }

    pub async fn draft_decision(
        &self,
        context: &DecisionContext,
        analysis: &str,
    ) -> Result<ParsedDecision> {
        let prompt = json!({
            "analysis": analysis,
            "context": context.to_prompt_value(),
            "output_contract": {
                "action": "open|add|reduce|close|hold",
                "side": "buy|sell",
                "target_quantity": "non-negative integer multiple of 100",
                "rationale": "short audit explanation",
                "risk_notes": "short risk comments"
            }
        });
        let raw = self
            .llm
            .chat(
                "You are the trader agent. Choose one action, side, and target quantity. Return JSON only.",
                &serde_json::to_string_pretty(&prompt)?,
                Some(&decision_json_schema()),
            )
            .await?;
        parse_decision(&raw)
    }
}

#[derive(Debug, Clone)]
pub struct RiskAgent {
    config: RiskConfig,
}

impl RiskAgent {
    pub fn new(config: RiskConfig) -> Self {
        Self { config }
    }

    pub fn apply(&self, context: &DecisionContext, draft: ParsedDecision) -> RiskDecision {
        let mut checks = Vec::new();
        let mut adjusted = draft;

        if self.config.blacklist.contains(&context.signal.symbol) {
            checks.push(RiskCheckResult {
                rule: "blacklist".to_owned(),
                passed: false,
                detail: format!("{} is blacklisted", context.signal.symbol),
            });
            adjusted.action = DecisionAction::Hold;
            adjusted.target_quantity = 0;
            adjusted.risk_notes = append_note(&adjusted.risk_notes, "blacklist veto");
            return RiskDecision {
                decision: adjusted,
                checks,
            };
        }
        checks.push(RiskCheckResult {
            rule: "blacklist".to_owned(),
            passed: true,
            detail: "symbol not blacklisted".to_owned(),
        });

        if matches!(adjusted.action, DecisionAction::Open | DecisionAction::Add) {
            let current_symbol = context.current_symbol_quantity();
            let allowed_single = (self.config.max_single_name_quantity - current_symbol).max(0);
            if adjusted.target_quantity > allowed_single {
                let clamped = clamp_to_lot(allowed_single);
                checks.push(RiskCheckResult {
                    rule: "single_name_concentration".to_owned(),
                    passed: clamped == adjusted.target_quantity,
                    detail: format!(
                        "requested {}, current {}, cap {}, adjusted {}",
                        adjusted.target_quantity,
                        current_symbol,
                        self.config.max_single_name_quantity,
                        clamped
                    ),
                });
                adjusted.target_quantity = clamped;
            } else {
                checks.push(RiskCheckResult {
                    rule: "single_name_concentration".to_owned(),
                    passed: true,
                    detail: "within single-name quantity cap".to_owned(),
                });
            }

            let total_allowed =
                (self.config.max_total_quantity - context.total_open_quantity()).max(0);
            if adjusted.target_quantity > total_allowed {
                let clamped = clamp_to_lot(total_allowed);
                checks.push(RiskCheckResult {
                    rule: "total_exposure".to_owned(),
                    passed: clamped == adjusted.target_quantity,
                    detail: format!(
                        "requested {}, open {}, cap {}, adjusted {}",
                        adjusted.target_quantity,
                        context.total_open_quantity(),
                        self.config.max_total_quantity,
                        clamped
                    ),
                });
                adjusted.target_quantity = clamped;
            } else {
                checks.push(RiskCheckResult {
                    rule: "total_exposure".to_owned(),
                    passed: true,
                    detail: "within total quantity cap".to_owned(),
                });
            }

            if adjusted.target_quantity == 0 {
                adjusted.action = DecisionAction::Hold;
                adjusted.risk_notes = append_note(&adjusted.risk_notes, "risk clamp to zero");
            }
        }

        RiskDecision {
            decision: adjusted,
            checks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskDecision {
    pub decision: ParsedDecision,
    pub checks: Vec<RiskCheckResult>,
}

pub struct DecisionOrchestrator<C> {
    llm: Arc<C>,
    analyst: AnalystAgent<C>,
    trader: TraderAgent<C>,
    risk: RiskAgent,
    availability: Arc<LlmAvailability>,
    logger: Arc<dyn DecisionLogger>,
}

impl<C> DecisionOrchestrator<C>
where
    C: LlmClient + 'static,
{
    pub fn new(llm: Arc<C>, config: OrchestratorConfig) -> Self {
        Self::with_logger(
            llm,
            config,
            Arc::new(LlmAvailability::new(true)),
            Arc::new(NoopDecisionLogger),
        )
    }

    pub fn with_logger(
        llm: Arc<C>,
        config: OrchestratorConfig,
        availability: Arc<LlmAvailability>,
        logger: Arc<dyn DecisionLogger>,
    ) -> Self {
        Self {
            analyst: AnalystAgent::new(llm.clone()),
            trader: TraderAgent::new(llm.clone()),
            risk: RiskAgent::new(config.risk),
            llm,
            availability,
            logger,
        }
    }

    pub async fn decide(&self, context: DecisionContext) -> Result<Decision> {
        if let Err(error) = self.llm.probe().await {
            self.availability.mark_unavailable();
            let decision = fallback_hold(&context, format!("LLM unavailable: {error}"));
            self.logger
                .save(
                    &decision,
                    None,
                    Some(json!({"source": "fallback_rule", "reason": error.to_string()})),
                    "fallback_rule",
                )
                .await?;
            return Ok(decision);
        }
        self.availability.mark_available();

        let analysis = match self.analyst.analyze(&context).await {
            Ok(analysis) => analysis,
            Err(error) => {
                self.availability.mark_unavailable();
                let decision = fallback_hold(&context, format!("LLM analyst failed: {error}"));
                self.logger
                    .save(
                        &decision,
                        None,
                        Some(json!({"source": "fallback_rule", "reason": error.to_string()})),
                        "fallback_rule",
                    )
                    .await?;
                return Ok(decision);
            }
        };

        let draft = match self.trader.draft_decision(&context, &analysis).await {
            Ok(draft) => draft,
            Err(error) => {
                let decision =
                    fallback_hold(&context, format!("LLM trader output invalid: {error}"));
                self.logger
                    .save(
                        &decision,
                        Some(json!({"analysis": analysis})),
                        Some(json!({"source": "fallback_rule", "reason": error.to_string()})),
                        "fallback_rule",
                    )
                    .await?;
                return Ok(decision);
            }
        };

        let risked = self.risk.apply(&context, draft);
        let decision = build_decision(&context, risked.decision, risked.checks);
        self.logger
            .save(
                &decision,
                Some(json!({"analysis": analysis})),
                Some(json!({"source": "llm"})),
                "llm",
            )
            .await?;
        Ok(decision)
    }
}

#[async_trait]
pub trait DecisionEngine: Send + Sync {
    async fn decide(&self, context: DecisionContext) -> Result<Decision>;
}

#[async_trait]
impl<C> DecisionEngine for DecisionOrchestrator<C>
where
    C: LlmClient + 'static,
{
    async fn decide(&self, context: DecisionContext) -> Result<Decision> {
        DecisionOrchestrator::decide(self, context).await
    }
}

fn build_decision(
    context: &DecisionContext,
    parsed: ParsedDecision,
    risk_checks: Vec<RiskCheckResult>,
) -> Decision {
    Decision {
        id: next_decision_id(),
        signal_id: Some(context.signal.id.clone()),
        symbol: context.signal.symbol.clone(),
        exchange: context.signal.exchange,
        action: parsed.action,
        side: parsed.side,
        target_quantity: parsed.target_quantity,
        rationale: if parsed.risk_notes.is_empty() {
            parsed.rationale
        } else {
            format!("{} Risk notes: {}", parsed.rationale, parsed.risk_notes)
        },
        risk_checks,
        ts: Utc::now(),
    }
}

fn fallback_hold(context: &DecisionContext, rationale: String) -> Decision {
    Decision {
        id: next_decision_id(),
        signal_id: Some(context.signal.id.clone()),
        symbol: context.signal.symbol.clone(),
        exchange: context.signal.exchange,
        action: DecisionAction::Hold,
        side: OrderSide::Buy,
        target_quantity: 0,
        rationale: format!(
            "degraded mode: {rationale}; holding position and opening no new positions"
        ),
        risk_checks: vec![RiskCheckResult {
            rule: "llm_availability".to_owned(),
            passed: false,
            detail: "ADR-020 fallback: Hold and no new positions".to_owned(),
        }],
        ts: Utc::now(),
    }
}

fn append_note(existing: &str, note: &str) -> String {
    if existing.trim().is_empty() {
        note.to_owned()
    } else {
        format!("{existing}; {note}")
    }
}

fn next_decision_id() -> String {
    let millis = Utc::now().timestamp_millis().max(0) as u64;
    let seq = DECISION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let process = u64::from(std::process::id());
    let value = ((millis as u128) << 80) | ((process as u128 & 0xffff) << 64) | u128::from(seq);
    encode_crockford_128(value)
}

fn encode_crockford_128(mut value: u128) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut out = [b'0'; 26];
    for idx in (0..26).rev() {
        out[idx] = ALPHABET[(value & 0x1f) as usize];
        value >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("Crockford alphabet is valid UTF-8")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{NaiveDate, TimeZone, Utc};
    use rust_decimal::Decimal;
    use tg_contracts::{
        DecisionAction, Exchange, OrderSide, Position, Signal, SignalDirection, StrategyStyle,
    };

    use super::{DecisionOrchestrator, OrchestratorConfig, RiskAgent, RiskConfig};
    use crate::context::{ContextPayload, DecisionContext};
    use crate::llm::MockLlmClient;
    use crate::schema::ParsedDecision;

    fn signal() -> Signal {
        Signal {
            id: "sig-1".to_owned(),
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            direction: SignalDirection::Long,
            strength: 0.8,
            confidence: 0.7,
            style: StrategyStyle::Swing,
            reason: vec!["indicator:RSI".to_owned()],
            suggested_quantity: Some(200),
            ts: Utc.with_ymd_and_hms(2026, 6, 15, 2, 0, 0).unwrap(),
            trading_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
        }
    }

    fn context() -> DecisionContext {
        DecisionContext::new(signal(), ContextPayload::default())
    }

    #[tokio::test]
    async fn orchestrator_returns_open_buy_decision_from_mock_llm() {
        let llm = Arc::new(MockLlmClient::new(vec![
            r#"{"summary":"bullish"}"#.to_owned(),
            r#"{"action":"open","side":"buy","target_quantity":200,"rationale":"signal confirmed","risk_notes":"ok"}"#.to_owned(),
        ]));
        let orchestrator = DecisionOrchestrator::new(llm, OrchestratorConfig::default());

        let decision = orchestrator.decide(context()).await.expect("decision");
        assert_eq!(decision.action, DecisionAction::Open);
        assert_eq!(decision.side, OrderSide::Buy);
        assert_eq!(decision.target_quantity, 200);
        assert!(decision.rationale.contains("signal confirmed"));
    }

    #[test]
    fn risk_agent_clamps_and_downgrades_when_concentration_violated() {
        let mut payload = ContextPayload::default();
        payload.positions.push(Position {
            symbol: "600519".to_owned(),
            exchange: Exchange::Sh,
            total_quantity: 900,
            t1_locked_quantity: 0,
            available_quantity: 900,
            avg_cost: Decimal::new(1000, 2),
            last_price: Decimal::new(1000, 2),
            market_value: Decimal::new(900000, 2),
            unrealized_pnl: Decimal::ZERO,
        });
        let context = DecisionContext::new(signal(), payload);
        let risk = RiskAgent::new(RiskConfig {
            max_single_name_quantity: 950,
            max_total_quantity: 10_000,
            blacklist: vec![],
        });
        let result = risk.apply(
            &context,
            ParsedDecision {
                action: DecisionAction::Open,
                side: OrderSide::Buy,
                target_quantity: 200,
                rationale: "buy".to_owned(),
                risk_notes: String::new(),
            },
        );

        assert_eq!(result.decision.action, DecisionAction::Hold);
        assert_eq!(result.decision.target_quantity, 0);
        assert!(result.checks.iter().any(|check| !check.passed));
    }

    #[tokio::test]
    async fn fallback_hold_when_probe_fails_then_recovers() {
        let llm = Arc::new(MockLlmClient::new(vec![
            r#"{"summary":"recovered"}"#.to_owned(),
            r#"{"action":"open","side":"buy","target_quantity":200,"rationale":"normal","risk_notes":"ok"}"#.to_owned(),
        ]));
        llm.set_probe_result(Err("down".to_owned())).await;
        let orchestrator = DecisionOrchestrator::new(llm.clone(), OrchestratorConfig::default());

        let fallback = orchestrator.decide(context()).await.expect("fallback");
        assert_eq!(fallback.action, DecisionAction::Hold);
        assert_eq!(fallback.target_quantity, 0);
        assert!(fallback.rationale.contains("degraded mode"));

        llm.set_probe_result(Ok(())).await;
        let recovered = orchestrator.decide(context()).await.expect("recovered");
        assert_eq!(recovered.action, DecisionAction::Open);
        assert_eq!(recovered.target_quantity, 200);
    }

    #[test]
    fn cost_precision_guard_has_no_forbidden_float_money_path_markers() {
        let sources = [
            include_str!("agents.rs"),
            include_str!("schema.rs"),
            include_str!("context.rs"),
        ]
        .join("\n");
        let float_cast = ["as", "f64"].join(" ");
        let quantity_cast = ["target_quantity", "as"].join(" ");
        assert!(!sources.contains(&float_cast));
        assert!(!sources.contains(&quantity_cast));
    }
}
