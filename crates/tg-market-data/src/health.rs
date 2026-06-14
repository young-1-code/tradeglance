use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::PgPool;

use crate::sidecar::SidecarClient;

#[derive(Clone)]
pub struct HealthState {
    pub sidecar: Arc<dyn SidecarClient>,
    pub pool: PgPool,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    sidecar: &'static str,
    database: &'static str,
}

pub fn health_router(state: HealthState) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

async fn health(State(state): State<HealthState>) -> impl IntoResponse {
    let sidecar_ok = state.sidecar.health().await.is_ok();
    let db_ok = sqlx::query("SELECT 1").execute(&state.pool).await.is_ok();
    let status = if sidecar_ok && db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    let body = HealthResponse {
        status: if status == StatusCode::OK {
            "ok"
        } else {
            "degraded"
        },
        sidecar: if sidecar_ok { "ok" } else { "error" },
        database: if db_ok { "ok" } else { "error" },
    };
    (status, Json(body))
}
