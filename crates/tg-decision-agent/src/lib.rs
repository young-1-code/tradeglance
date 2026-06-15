#![forbid(unsafe_code)]

pub mod agents;
pub mod context;
pub mod fallback;
pub mod grpc;
pub mod llm;
pub mod logging;
pub mod schema;

pub use agents::{
    AnalystAgent, DecisionOrchestrator, OrchestratorConfig, RiskAgent, RiskConfig, TraderAgent,
};
pub use context::{build_user_prompt, ContextPayload, DecisionContext};
pub use fallback::{spawn_probe_task, LlmAvailability};
pub use grpc::DecisionGrpcService;
pub use llm::{LlmClient, MockLlmClient, OpenAiCompatibleClient, OpenAiCompatibleConfig};
pub use logging::{DecisionLogger, NoopDecisionLogger, PersistenceDecisionLogger};
pub use schema::{decision_json_schema, parse_decision, ParsedDecision};
