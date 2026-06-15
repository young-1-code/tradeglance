use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tg_contracts::Decision;
use tg_persistence::{DecisionAuditRecord, DecisionRepo};

#[async_trait]
pub trait DecisionLogger: Send + Sync {
    async fn save(
        &self,
        decision: &Decision,
        analysis: Option<Value>,
        pipeline_meta: Option<Value>,
        source: &str,
    ) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoopDecisionLogger;

#[async_trait]
impl DecisionLogger for NoopDecisionLogger {
    async fn save(
        &self,
        _decision: &Decision,
        _analysis: Option<Value>,
        _pipeline_meta: Option<Value>,
        _source: &str,
    ) -> Result<()> {
        Ok(())
    }
}

pub struct PersistenceDecisionLogger<R> {
    repo: Arc<R>,
}

impl<R> PersistenceDecisionLogger<R> {
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl<R> DecisionLogger for PersistenceDecisionLogger<R>
where
    R: DecisionRepo + 'static,
{
    async fn save(
        &self,
        decision: &Decision,
        analysis: Option<Value>,
        pipeline_meta: Option<Value>,
        source: &str,
    ) -> Result<()> {
        self.repo
            .save_decision(DecisionAuditRecord {
                decision: decision.clone(),
                analysis,
                pipeline_meta,
                source: source.to_owned(),
            })
            .await
            .map_err(|error| anyhow::anyhow!(error))
    }
}
