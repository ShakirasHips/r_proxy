mod stations;

use axum::{
    routing::{get}, Router,
};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use proxy::HealthProber;

pub struct Outpost {
    listener: TcpListener,
}

impl Outpost {
    pub async fn bind(addr: SocketAddr) -> io::Result<Outpost> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("outpost listening on {}", listener.local_addr()?);
        Ok(Outpost { listener })
    }

    pub async fn serve(
        self,
        health_prober: Arc<HealthProber>,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> io::Result<()> {
        let app = Router::new()
            .route("/", get(Self::root))
            .merge(stations::health::health::router(health_prober))
            .layer(TraceLayer::new_for_http());

        axum::serve(self.listener, app)
            .with_graceful_shutdown(shutdown)
            .await
    }

    async fn root() -> &'static str {
        "Hello"
    }
}
