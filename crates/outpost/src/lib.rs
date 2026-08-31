mod stations;

use axum::{
    routing::{get}, Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[derive(Default)]
pub struct AppState {
    pub health: Arc<stations::health::state::HealthState>,
}

pub struct Outpost;

impl Outpost{
    pub async fn start() {
        tracing_subscriber::fmt::init();

        let state = Arc::new(AppState::default());
        let app = Router::new()
            .route("/", get(Self::root))
            .merge(stations::health::health::router(state.health.clone()))
            .layer(TraceLayer::new_for_http());

        let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
            .await
            .unwrap();
        tracing::info!("listening on {}", listener.local_addr().unwrap());
        axum::serve(listener, app).await.unwrap();
    }

    async fn root() -> &'static str {
        "Hello"
    }
}