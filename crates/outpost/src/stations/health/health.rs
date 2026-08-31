use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use std::sync::{atomic::Ordering, Arc};
use crate::stations::health::model::{HealthEntry, HealthQuery};
use crate::stations::health::state::HealthState;

async fn list_entries(
    State(state): State<Arc<HealthState>>,
    Query(params): Query<HealthQuery>,
) -> Json<Vec<i32>> {
    let entries = state.entries.lock().unwrap();

    let filtered: Vec<i32> = entries
        .iter()
        .cloned()
        .collect();

    Json(filtered)
}

pub fn router(state: Arc<HealthState>) -> Router {
    Router::new()
        .route("/health/entries", get(list_entries))
        .with_state(state)
}