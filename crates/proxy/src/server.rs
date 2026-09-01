use crate::config::Config;
use crate::health_prober::HealthProber;
use crate::proxy::Proxy;
use anyhow::{Context, Ok, Result, ensure};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

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

    pub fn start(&mut self) -> Result<()> {
        let total = self.proxies.len();
        let mut failures = vec![];

        for prox in self.proxies.iter_mut() {
            if let Err(e) = prox.start().context("failed to start proxy") {
                eprintln!("{e:#}");
                failures.push(e);
            }
        }

        if failures.len() == total && total > 0 {
            anyhow::bail!("all {total} proxies failed to start");
        }

        if !failures.is_empty() {
            eprintln!(
                "warning: {}/{} proxies failed to start, continuing with the rest",
                failures.len(),
                total
            );
        }

        Ok(())
    }

    pub async fn shutdown(mut self) -> Result<()> {
        for mut p in self.proxies.drain(..) {
            let _ = p.stop();
        }

        Ok(())
    }
}
