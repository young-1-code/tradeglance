#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tg_contracts::proto::tg::v1::decision_service_server::DecisionServiceServer;
use tg_decision_agent::{
    spawn_probe_task, DecisionGrpcService, DecisionOrchestrator, LlmAvailability,
    NoopDecisionLogger, OpenAiCompatibleClient, OpenAiCompatibleConfig, OrchestratorConfig,
};
use tonic::transport::Server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let addr: SocketAddr = std::env::var("TG_DECISION_AGENT_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50053".to_owned())
        .parse()?;
    let llm = Arc::new(OpenAiCompatibleClient::new(
        OpenAiCompatibleConfig::from_env()?,
    )?);
    let availability = Arc::new(LlmAvailability::new(true));
    let _probe = spawn_probe_task(
        llm.clone(),
        availability.clone(),
        std::time::Duration::from_secs(30),
    );
    let orchestrator = Arc::new(DecisionOrchestrator::with_logger(
        llm,
        OrchestratorConfig::default(),
        availability,
        Arc::new(NoopDecisionLogger),
    ));
    let service = DecisionGrpcService::new(orchestrator);

    tracing::info!(%addr, "starting tg-decision-agent");
    Server::builder()
        .add_service(DecisionServiceServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
