use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use std::sync::{atomic::Ordering, Arc};
use proxy::HealthProber;
use serde_json::json;

async fn get_health_for_id(
    State(state): State<Arc<HealthProber>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    match state.get_points(&id) {
        Some(points) => Json(json!({ "id": id, "data_points": points })),
        None => Json(json!({ "id": id, "data_points": [] })),
    }
}

async fn get_health_summary(State(prober): State<Arc<HealthProber>>) -> Json<serde_json::Value> {
    let summary = prober.summary();
    Json(json!({
        "totals": summary.into_iter()
            .map(|(id, total)| json!({ "id": id, "total_bytes_sent": total }))
            .collect::<Vec<_>>()
    }))
}

pub fn router(state: Arc<HealthProber>) -> Router {
    Router::new()
        .route("/health", get(get_health_summary))
        .route("/health/{id}", get(get_health_for_id))
        .with_state(state)
}