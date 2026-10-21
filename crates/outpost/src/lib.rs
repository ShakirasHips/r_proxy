mod stations;

use axum::{
    routing::{get}, Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use proxy::HealthProber;

pub struct Outpost;

impl Outpost{
    pub async fn start(health_prober : Arc<HealthProber>) {
        // tracing_subscriber::fmt::init();

        let app = Router::new()
            .route("/", get(Self::root))
            .merge(stations::health::health::router(health_prober))
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