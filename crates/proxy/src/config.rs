use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    LoadBalancer,
    ReverseProxy,
    Proxy,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default = "default_outpost_address")]
    pub outpost_address: SocketAddr,

    pub instances: Vec<Instance>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Instance {
    pub name: String,

    #[serde(default = "default_connect_timeout_secs")]
    pub connect_timeout_secs: u64,

    pub operating_mode: OperatingMode,

    #[serde(default)]
    pub ingress_addresses: Vec<SocketAddr>,

    #[serde(default)]
    pub egress_addresses: Vec<SocketAddr>,
}

fn default_outpost_address() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000)
}

fn default_connect_timeout_secs() -> u64 {
    30
}

fn listen_conflict(a: &SocketAddr, b: &SocketAddr) -> bool {
    a.port() == b.port() && (a.ip() == b.ip() || a.ip().is_unspecified() || b.ip().is_unspecified())
}

fn routes_to(egress: &SocketAddr, ingress: &SocketAddr) -> bool {
    egress.port() == ingress.port()
        && (egress.ip() == ingress.ip()
            || (ingress.ip().is_unspecified()
                && (egress.ip().is_loopback() || egress.ip().is_unspecified())))
}

fn validate_config(config: &Config) -> Result<()> {
    let mut names = HashSet::new();
    for instance in &config.instances {
        if instance.name.is_empty() {
            bail!("Instance names must not be empty");
        }
        if !names.insert(instance.name.as_str()) {
            bail!("Duplicate instance name '{}'", instance.name);
        }
        if instance.operating_mode == OperatingMode::LoadBalancer {
            bail!("'{}': LoadBalancer mode is not implemented yet", instance.name);
        }
        if instance.ingress_addresses.is_empty() {
            bail!("'{}' has no ingress_addresses", instance.name);
        }
        if instance.egress_addresses.is_empty() {
            bail!("'{}' has no egress_addresses", instance.name);
        }
        if instance.connect_timeout_secs == 0 {
            bail!("'{}': connect_timeout_secs must be greater than 0", instance.name);
        }
        if let Some(addr) = instance.ingress_addresses.iter().find(|x| x.port() < 1024) {
            bail!("'{}': ingress port {} is below 1024", instance.name, addr.port());
        }
    }

    let ingress: Vec<(&str, &SocketAddr)> = config
        .instances
        .iter()
        .flat_map(|i| i.ingress_addresses.iter().map(move |a| (i.name.as_str(), a)))
        .collect();

    for (idx, (name, addr)) in ingress.iter().enumerate() {
        if let Some((other, other_addr)) = ingress[idx + 1..]
            .iter()
            .find(|(_, b)| listen_conflict(addr, b))
        {
            bail!("Ingress {addr} on '{name}' conflicts with {other_addr} on '{other}'");
        }
    }

    for instance in &config.instances {
        for egress in &instance.egress_addresses {
            if let Some((other, addr)) = ingress.iter().find(|(_, i)| routes_to(egress, i)) {
                bail!(
                    "Egress {egress} on '{}' points back at ingress {addr} on '{other}'",
                    instance.name
                );
            }
        }
    }

    Ok(())
}

impl Config {
    pub fn build(path: &str) -> Result<Config> {
        let contents =
            fs::read_to_string(path).with_context(|| format!("Failed to read config {path}"))?;
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

    fn instance(name: &str, ingress: &[&str], egress: &[&str]) -> String {
        let list = |xs: &[&str]| {
            xs.iter()
                .map(|x| format!("\"{x}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        format!(
            "[[instances]]\nname = \"{name}\"\noperating_mode = \"Proxy\"\ningress_addresses = [{}]\negress_addresses = [{}]\n",
            list(ingress),
            list(egress)
        )
    }

    fn validate(instances: &[String]) -> Result<()> {
        validate_config(&parse(&instances.concat()))
    }

    fn assert_err(instances: &[String], needle: &str) {
        let err = validate(instances).expect_err("expected validation to fail");
        assert!(err.to_string().contains(needle), "unexpected error: {err}");
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
            connect_timeout_secs = 5
        },
        ]"#,
        );
        let first = cfg.instances.first().unwrap();
        assert_eq!(first.operating_mode, OperatingMode::Proxy);
        assert_eq!(first.ingress_addresses[0].port(), 8080);
        assert_eq!(first.egress_addresses[0].port(), 9090);
        assert_eq!(first.connect_timeout_secs, 5);
        assert_eq!(cfg.outpost_address, default_outpost_address());
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_defaults() {
        let cfg = parse(&instance("A", &["127.0.0.1:8080"], &["127.0.0.1:9090"]));
        assert_eq!(cfg.instances[0].connect_timeout_secs, 30);
    }

    #[test]
    fn test_init_config() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test.toml");
        let cfg = Config::build(path).unwrap();
        let first = cfg.instances.first().unwrap();
        assert_eq!(first.operating_mode, OperatingMode::Proxy);
        assert_eq!(first.ingress_addresses[0].port(), 8080);
        assert_eq!(first.egress_addresses[0].port(), 9090);
    }

    #[test]
    fn test_same_port_different_ip_is_ok() {
        assert!(
            validate(&[
                instance("A", &["127.0.0.1:8080"], &["127.0.0.1:9090"]),
                instance("B", &["127.0.0.2:8080"], &["127.0.0.1:9091"]),
            ])
            .is_ok()
        );
    }

    #[test]
    fn test_shared_and_privileged_egress_is_ok() {
        assert!(
            validate(&[
                instance("A", &["127.0.0.1:8080"], &["10.0.0.5:80"]),
                instance("B", &["127.0.0.1:8081"], &["10.0.0.5:80"]),
            ])
            .is_ok()
        );
    }

    #[test]
    fn test_remote_egress_on_ingress_port_is_ok() {
        assert!(validate(&[instance("A", &["127.0.0.1:8080"], &["10.0.0.5:8080"])]).is_ok());
    }

    #[test]
    fn test_duplicate_ingress_within_instance() {
        assert_err(
            &[instance("A", &["127.0.0.1:8080", "127.0.0.1:8080"], &["127.0.0.1:9090"])],
            "conflicts",
        );
    }

    #[test]
    fn test_ingress_conflict_across_instances() {
        assert_err(
            &[
                instance("A", &["127.0.0.1:8080"], &["127.0.0.1:9090"]),
                instance("B", &["127.0.0.1:8080"], &["127.0.0.1:9091"]),
            ],
            "conflicts",
        );
    }

    #[test]
    fn test_wildcard_ingress_conflict() {
        assert_err(
            &[
                instance("A", &["0.0.0.0:8080"], &["127.0.0.1:9090"]),
                instance("B", &["127.0.0.1:8080"], &["127.0.0.1:9091"]),
            ],
            "conflicts",
        );
    }

    #[test]
    fn test_egress_loops_to_ingress() {
        assert_err(
            &[
                instance("A", &["0.0.0.0:8080"], &["127.0.0.1:9090"]),
                instance("B", &["127.0.0.1:8081"], &["127.0.0.1:8080"]),
            ],
            "points back",
        );
    }

    #[test]
    fn test_duplicate_names() {
        assert_err(
            &[
                instance("A", &["127.0.0.1:8080"], &["127.0.0.1:9090"]),
                instance("A", &["127.0.0.1:8081"], &["127.0.0.1:9091"]),
            ],
            "Duplicate instance name",
        );
    }

    #[test]
    fn test_privileged_ingress() {
        assert_err(&[instance("A", &["127.0.0.1:80"], &["127.0.0.1:9090"])], "below 1024");
    }

    #[test]
    fn test_missing_egress() {
        assert_err(&[instance("A", &["127.0.0.1:8080"], &[])], "no egress_addresses");
    }

    #[test]
    fn test_load_balancer_rejected() {
        let toml = instance("A", &["127.0.0.1:8080"], &["127.0.0.1:9090"])
            .replace("\"Proxy\"", "\"LoadBalancer\"");
        assert_err(&[toml], "not implemented");
    }
}
