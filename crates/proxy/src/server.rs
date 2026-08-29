use crate::config::Config;
use crate::health_prober::HealthProber;
use crate::proxy::Proxy;
use anyhow::{Context, Ok, Result, ensure};
use std::sync::Arc;
use std::io::{self, Read};
use std::thread;

pub struct Server {
    config: Config,
    proxies: Vec<Proxy>,
    health_prober: Arc<HealthProber>,
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

        let health_printer = self.health_prober.clone();
        thread::spawn(move || {
            loop {
                health_printer.print();
                thread::sleep(std::time::Duration::from_secs(1));
            }
        });

        Ok(())
    }

    pub fn run(&mut self) -> Result<()>
    {
        println!("Press Enter to stop...");
        let mut buf = String::new();
        io::stdin().read_line(&mut buf).unwrap();
        
        Ok(())
    }

    pub fn stop(mut self) {
        for mut p in self.proxies.drain(..) {
            let _ = p.stop();
        }
    }
}
