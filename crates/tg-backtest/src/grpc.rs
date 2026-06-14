use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use chrono::Utc;
use tg_contracts::proto::tg::v1::backtest_service_server::BacktestService as ProtoBacktestService;
use tg_contracts::proto::tg::v1::{
    BacktestJob, BacktestResult, BacktestStatus, GetBacktestResultRequest,
    GetBacktestStatusRequest, SubmitBacktestRequest,
};
use tonic::{Request, Response, Status};

use crate::perf::BacktestMetrics;

static JOB_COUNTER: AtomicU64 = AtomicU64::new(1);

#[async_trait]
pub trait BacktestJobExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        run_id: String,
        request: SubmitBacktestRequest,
    ) -> Result<BacktestMetrics, anyhow::Error>;
}

#[derive(Debug, Clone)]
pub struct JobSnapshot {
    pub id: String,
    pub status: String,
    pub progress: f64,
    pub error: Option<String>,
    pub metrics_json: Option<String>,
    pub created_at_epoch_millis: i64,
}

#[derive(Debug)]
struct JobState {
    status: String,
    progress: f64,
    error: Option<String>,
    metrics_json: Option<String>,
    created_at_epoch_millis: i64,
}

#[derive(Clone)]
pub struct InProcessBacktestService {
    executor: Arc<dyn BacktestJobExecutor>,
    jobs: Arc<RwLock<HashMap<String, JobState>>>,
}

impl InProcessBacktestService {
    pub fn new(executor: Arc<dyn BacktestJobExecutor>) -> Self {
        Self {
            executor,
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn snapshot(&self, id: &str) -> Option<JobSnapshot> {
        self.jobs
            .read()
            .expect("backtest job table lock should not be poisoned")
            .get(id)
            .map(|state| JobSnapshot {
                id: id.to_owned(),
                status: state.status.clone(),
                progress: state.progress,
                error: state.error.clone(),
                metrics_json: state.metrics_json.clone(),
                created_at_epoch_millis: state.created_at_epoch_millis,
            })
    }
}

#[async_trait]
impl ProtoBacktestService for InProcessBacktestService {
    async fn submit_backtest(
        &self,
        request: Request<SubmitBacktestRequest>,
    ) -> Result<Response<BacktestJob>, Status> {
        let request = request.into_inner();
        if request.symbols.is_empty() {
            return Err(Status::invalid_argument("symbols must not be empty"));
        }
        if request.start_epoch_millis >= request.end_epoch_millis {
            return Err(Status::invalid_argument("start must be before end"));
        }

        let id = new_job_id();
        let created_at_epoch_millis = Utc::now().timestamp_millis();
        self.jobs
            .write()
            .map_err(|_| Status::internal("backtest job table lock poisoned"))?
            .insert(
                id.clone(),
                JobState {
                    status: "pending".to_owned(),
                    progress: 0.0,
                    error: None,
                    metrics_json: None,
                    created_at_epoch_millis,
                },
            );

        let jobs = Arc::clone(&self.jobs);
        let executor = Arc::clone(&self.executor);
        let run_id = id.clone();
        tokio::spawn(async move {
            update_job(&jobs, &run_id, "running", 0.0, None, None);
            match executor.execute(run_id.clone(), request).await {
                Ok(metrics) => match serde_json::to_string(&metrics) {
                    Ok(metrics_json) => {
                        update_job(&jobs, &run_id, "done", 1.0, None, Some(metrics_json));
                    }
                    Err(error) => {
                        update_job(&jobs, &run_id, "failed", 1.0, Some(error.to_string()), None);
                    }
                },
                Err(error) => {
                    update_job(&jobs, &run_id, "failed", 1.0, Some(error.to_string()), None);
                }
            }
        });

        Ok(Response::new(BacktestJob {
            id,
            status: "pending".to_owned(),
            created_at_epoch_millis,
        }))
    }

    async fn get_backtest_status(
        &self,
        request: Request<GetBacktestStatusRequest>,
    ) -> Result<Response<BacktestStatus>, Status> {
        let id = request.into_inner().id;
        let snapshot = self
            .snapshot(&id)
            .ok_or_else(|| Status::not_found(format!("backtest job {id} not found")))?;
        Ok(Response::new(BacktestStatus {
            id: snapshot.id,
            status: snapshot.status,
            progress: snapshot.progress,
            error: snapshot.error.unwrap_or_default(),
        }))
    }

    async fn get_backtest_result(
        &self,
        request: Request<GetBacktestResultRequest>,
    ) -> Result<Response<BacktestResult>, Status> {
        let id = request.into_inner().id;
        let snapshot = self
            .snapshot(&id)
            .ok_or_else(|| Status::not_found(format!("backtest job {id} not found")))?;
        if snapshot.status != "done" {
            return Err(Status::failed_precondition(format!(
                "backtest job {} is {}",
                snapshot.id, snapshot.status
            )));
        }
        Ok(Response::new(BacktestResult {
            id: snapshot.id,
            status: snapshot.status,
            metrics_json: snapshot.metrics_json.unwrap_or_default(),
        }))
    }
}

fn update_job(
    jobs: &RwLock<HashMap<String, JobState>>,
    id: &str,
    status: &str,
    progress: f64,
    error: Option<String>,
    metrics_json: Option<String>,
) {
    if let Ok(mut jobs) = jobs.write() {
        if let Some(job) = jobs.get_mut(id) {
            job.status = status.to_owned();
            job.progress = progress;
            job.error = error;
            job.metrics_json = metrics_json;
        }
    }
}

fn new_job_id() -> String {
    format!(
        "BT{:013}{:08}",
        Utc::now().timestamp_millis().max(0),
        JOB_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
