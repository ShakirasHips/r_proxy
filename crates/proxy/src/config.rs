#![feature(test)]
use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;

#[derive(Deserialize, Debug, Clone)]
pub enum OperatingMode {
    LoadBalancer,
    ReverseProxy,
    Proxy,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub instances: Vec<Instance>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Instance {
    pub name: String,

    pub ttl: u64,

    pub operating_mode: OperatingMode,

    #[serde(default)]
    pub ingress_addresses: Vec<SocketAddr>,

    #[serde(default)]
    pub egress_addresses: Vec<SocketAddr>,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            name: "".to_string(),
            ttl: 30000,
            operating_mode: OperatingMode::Proxy,
            ingress_addresses: vec![],
            egress_addresses: vec![],
        }
    }
}

fn validate_config(config: &Config) -> Result<()> {
    let mut ingress_owner: HashMap<u16, &str> = HashMap::new();
    for instance in &config.instances {
        for addr in &instance.ingress_addresses {
            if let Some(other) = ingress_owner.insert(addr.port(), instance.name.as_str()) {
                if other != instance.name {
                    return Err(anyhow!(
                        "Ingress port {} on '{}' conflicts with '{}'",
                        addr.port(),
                        instance.name,
                        other
                    ));
                }
            }
        }
    }

    let mut egress_owner: HashMap<u16, &str> = HashMap::new();
    for instance in &config.instances {
        for addr in &instance.egress_addresses {
            if let Some(other) = egress_owner.insert(addr.port(), instance.name.as_str()) {
                if other != instance.name {
                    return Err(anyhow!(
                        "Egress port {} on '{}' conflicts with '{}'",
                        addr.port(),
                        instance.name,
                        other
                    ));
                }
            }
        }
    }

    for instance in &config.instances {
        if let Some(overlap) = instance
            .egress_addresses
            .iter()
            .find(|x| ingress_owner.contains_key(&x.port()))
        {
            return Err(anyhow!(
                "Ingress and egress port overlap at {}",
                overlap.port()
            ));
        }

        if instance.ingress_addresses.iter().any(|x| x.port() < 1024) {
            return Err(anyhow!("Ingress ports shouldn't be below 1024"));
        }

        if instance.egress_addresses.iter().any(|x| x.port() < 1024) {
            return Err(anyhow!("Egress ports shouldn't be below 1024"));
        }
    }

    Ok(())
}

impl Config {
    pub fn build(path: &str) -> Result<Config> {
        let contents = fs::read_to_string(path).with_context(|| "Failed to read config")?;
        let config: Config = toml::from_str(&contents).with_context(|| "Failed to parse config")?;

        validate_config(&config)?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Config {
        toml::from_str(toml).expect("Failed to parse test config")
    }

    #[test]
    fn test_valid_config() {
        let cfg = parse(
            r#"instances = [
        {
            name = "Test",
            operating_mode = "Proxy",
            ingress_addresses = ["127.0.0.1:8080", "127.0.0.1:8081"],
            egress_addresses  = ["127.0.0.1:9090"],
        },
        ]"#,
        );
        assert!(matches!(
            cfg.instances.first().unwrap().operating_mode,
            OperatingMode::Proxy
        ));
        assert_eq!(
            cfg.instances.first().unwrap().ingress_addresses[0].port(),
            8080
        );
        assert_eq!(
            cfg.instances.first().unwrap().egress_addresses[0].port(),
            9090
        );
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_init_config() {
        let cfg = Config::build("./test.toml").unwrap();
        assert!(matches!(
            cfg.instances.first().unwrap().operating_mode,
            OperatingMode::Proxy
        ));
        assert_eq!(
            cfg.instances.first().unwrap().ingress_addresses[0].port(),
            8080
        );
        assert_eq!(
            cfg.instances.first().unwrap().egress_addresses[0].port(),
            9090
        );
        assert!(validate_config(&cfg).is_ok());
    }
}
