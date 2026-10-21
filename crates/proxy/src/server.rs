use crate::config::Config;
use crate::health_prober::HealthProber;
use crate::proxy::Proxy;
use anyhow::{Context, Ok, Result};
use std::net::SocketAddr;
use std::sync::Arc;

pub struct Server {
    config: Config,
    proxies: Vec<Proxy>,
    pub health_prober: Arc<HealthProber>,
}

impl Server {
    pub fn build(config_path: &str) -> Result<Server> {
        let config = Config::build(config_path).with_context(|| "Failed to load config")?;
        let health_prober = Arc::new(HealthProber::new());
        let proxies = config
            .instances
            .iter()
            .map(|x| Proxy::build(x, health_prober.clone()))
            .collect::<Vec<Proxy>>();

        Ok(Server {
            config,
            proxies,
            health_prober,
        })
    }

    pub fn outpost_address(&self) -> SocketAddr {
        self.config.outpost_address
    }

    pub async fn start(&mut self) -> Result<()> {
        let total = self.proxies.len();
        let mut failures = vec![];

        for prox in self.proxies.iter_mut() {
            if let Err(e) = prox.start().await.context("failed to start proxy") {
                log::error!("{e:#}");
                failures.push(e);
            }
        }

        if failures.len() == total && total > 0 {
            anyhow::bail!("all {total} proxies failed to start");
        }

        if !failures.is_empty() {
            log::warn!(
                "{}/{} proxies failed to start, continuing with the rest",
                failures.len(),
                total
            );
        }

        Ok(())
    }

    pub async fn shutdown(self) -> Result<()> {
        for p in &self.proxies {
            p.stop().await;
        }

        Ok(())
    }
}
