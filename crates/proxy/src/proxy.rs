use crate::config::{Instance, OperatingMode};
use crate::health_prober::{HealthDataPoint, HealthProber, InstanceCounters};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

const BUF_SIZE: usize = 4096;

struct ProxyContext {
    name: String,
    mode: OperatingMode,
    egress_addresses: Vec<SocketAddr>,
    connect_timeout: Duration,
    health_prober: Arc<HealthProber>,
    counters: Arc<InstanceCounters>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
}

pub struct Proxy {
    ingress_addresses: Vec<SocketAddr>,
    local_addrs: Vec<SocketAddr>,
    ctx: Arc<ProxyContext>,
}

impl Proxy {
    pub fn build(instance: &Instance, health_prober: Arc<HealthProber>) -> Proxy {
        Proxy {
            ingress_addresses: instance.ingress_addresses.clone(),
            local_addrs: Vec::new(),
            ctx: Arc::new(ProxyContext {
                name: instance.name.clone(),
                mode: instance.operating_mode,
                egress_addresses: instance.egress_addresses.clone(),
                connect_timeout: Duration::from_secs(instance.connect_timeout_secs),
                counters: health_prober.counters(&instance.name),
                health_prober,
                shutdown: CancellationToken::new(),
                tasks: TaskTracker::new(),
            }),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        let mut listeners = Vec::new();
        for addr in &self.ingress_addresses {
            let listener = TcpListener::bind(addr)
                .await
                .with_context(|| format!("'{}': failed to bind listener on {addr}", self.ctx.name))?;
            listeners.push((*addr, listener));
        }

        for (addr, listener) in listeners {
            self.local_addrs.push(listener.local_addr()?);
            log::info!("'{}': listening on {addr}", self.ctx.name);
            self.ctx
                .tasks
                .spawn(Self::accept_loop(listener, addr, self.ctx.clone()));
        }
        Ok(())
    }

    #[cfg(test)]
    fn local_addrs(&self) -> &[SocketAddr] {
        &self.local_addrs
    }

    pub async fn stop(&self) {
        self.ctx.shutdown.cancel();
        self.ctx.tasks.close();
        self.ctx.tasks.wait().await;
    }

    async fn accept_loop(listener: TcpListener, ingress: SocketAddr, ctx: Arc<ProxyContext>) {
        loop {
            tokio::select! {
                _ = ctx.shutdown.cancelled() => break,
                res = listener.accept() => match res {
                    Ok((stream, _)) => {
                        ctx.tasks.spawn(Self::handle_connection(stream, ingress, ctx.clone()));
                    }
                    Err(e) => {
                        log::error!("'{}': accept error on {ingress}: {e}", ctx.name);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
    }

    async fn handle_connection(stream: TcpStream, ingress: SocketAddr, ctx: Arc<ProxyContext>) {
        let _guard = ctx.counters.open_connection();
        let mut bytes_sent = 0;

        let result = tokio::select! {
            _ = ctx.shutdown.cancelled() => Ok(()),
            res = Self::relay(stream, &ctx, &mut bytes_sent) => res,
        };

        if let Err(e) = &result {
            log::error!("'{}': connection on {ingress}: {e:#}", ctx.name);
        }

        ctx.health_prober.insert(HealthDataPoint {
            id: ctx.name.clone(),
            ingress_addresses: vec![ingress.to_string()],
            egress_addresses: ctx.egress_addresses.iter().map(|a| a.to_string()).collect(),
            bytes_sent,
            error: result.err().map(|e| format!("{e:#}")),
            timestamp: Utc::now(),
        });
    }

    async fn relay(inbound: TcpStream, ctx: &ProxyContext, bytes_sent: &mut u64) -> Result<()> {
        let outbound = Self::connect_egress(ctx).await?;
        match ctx.mode {
            OperatingMode::ReverseProxy => {
                Self::relay_bidirectional(inbound, outbound, &ctx.counters, bytes_sent).await
            }
            OperatingMode::Proxy | OperatingMode::LoadBalancer => {
                let mut inbound = inbound;
                let mut outbound = outbound;
                Self::forward(&mut inbound, &mut outbound, &ctx.counters, bytes_sent).await
            }
        }
    }

    async fn connect_egress(ctx: &ProxyContext) -> Result<Vec<(SocketAddr, TcpStream)>> {
        let attempts: Vec<_> = ctx
            .egress_addresses
            .iter()
            .map(|&addr| {
                let timeout = ctx.connect_timeout;
                tokio::spawn(async move {
                    (addr, tokio::time::timeout(timeout, TcpStream::connect(addr)).await)
                })
            })
            .collect();

        let mut streams = Vec::new();
        for attempt in attempts {
            match attempt.await.context("egress connect task failed")? {
                (addr, Ok(Ok(stream))) => streams.push((addr, stream)),
                (addr, Ok(Err(e))) => log::warn!("'{}': failed to connect to {addr}: {e}", ctx.name),
                (addr, Err(_)) => log::warn!("'{}': timed out connecting to {addr}", ctx.name),
            }
        }

        if streams.is_empty() {
            bail!("none of the egress addresses could be reached");
        }
        Ok(streams)
    }

    async fn forward<R, W>(
        src: &mut R,
        dests: &mut [(SocketAddr, W)],
        counters: &InstanceCounters,
        bytes_sent: &mut u64,
    ) -> Result<()>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        let mut buf = [0u8; BUF_SIZE];
        loop {
            let n = src.read(&mut buf).await.context("failed reading from client")?;
            if n == 0 {
                break;
            }
            for (addr, dest) in dests.iter_mut() {
                dest.write_all(&buf[..n])
                    .await
                    .with_context(|| format!("failed writing to {addr}"))?;
            }
            *bytes_sent += n as u64;
            counters.add_bytes_sent(n as u64);
        }

        for (_, dest) in dests.iter_mut() {
            let _ = dest.shutdown().await;
        }
        Ok(())
    }

    async fn relay_bidirectional(
        inbound: TcpStream,
        outbound: Vec<(SocketAddr, TcpStream)>,
        counters: &InstanceCounters,
        bytes_sent: &mut u64,
    ) -> Result<()> {
        let (mut client_rd, client_wr) = inbound.into_split();
        let client_wr = Arc::new(Mutex::new(client_wr));

        let mut reverse = JoinSet::new();
        let mut egress_wr = Vec::new();
        for (addr, stream) in outbound {
            let (mut rd, wr) = stream.into_split();
            egress_wr.push((addr, wr));
            let client_wr = client_wr.clone();
            reverse.spawn(async move {
                let mut buf = [0u8; BUF_SIZE];
                loop {
                    let n = rd
                        .read(&mut buf)
                        .await
                        .with_context(|| format!("failed reading from {addr}"))?;
                    if n == 0 {
                        return Ok::<_, anyhow::Error>(());
                    }
                    client_wr
                        .lock()
                        .await
                        .write_all(&buf[..n])
                        .await
                        .context("failed writing back to client")?;
                }
            });
        }

        let forward = Self::forward(&mut client_rd, &mut egress_wr, counters, bytes_sent);
        let backward = async {
            while let Some(res) = reverse.join_next().await {
                res.context("reverse relay task failed")??;
            }
            let _ = client_wr.lock().await.shutdown().await;
            Ok(())
        };

        tokio::try_join!(forward, backward)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(mode: OperatingMode, egress: Vec<SocketAddr>) -> Instance {
        Instance {
            name: "T".to_string(),
            connect_timeout_secs: 1,
            operating_mode: mode,
            ingress_addresses: vec!["127.0.0.1:0".parse().unwrap()],
            egress_addresses: egress,
        }
    }

    async fn backend() -> (TcpListener, SocketAddr) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        (listener, addr)
    }

    async fn read_all(listener: &TcpListener) -> Vec<u8> {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        buf
    }

    async fn wait_for_data_point(prober: &HealthProber) -> HealthDataPoint {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(p) = prober.get_points("T").and_then(|p| p.last().cloned()) {
                    return p;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("no data point recorded")
    }

    async fn started(mode: OperatingMode, egress: Vec<SocketAddr>) -> (Proxy, Arc<HealthProber>) {
        let prober = Arc::new(HealthProber::new());
        let mut proxy = Proxy::build(&instance(mode, egress), prober.clone());
        proxy.start().await.unwrap();
        (proxy, prober)
    }

    #[tokio::test]
    async fn test_forwards_to_every_egress_and_records_stats() {
        let (b1, a1) = backend().await;
        let (b2, a2) = backend().await;
        let (proxy, prober) = started(OperatingMode::Proxy, vec![a1, a2]).await;

        let mut client = TcpStream::connect(proxy.local_addrs()[0]).await.unwrap();
        client.write_all(b"hello").await.unwrap();
        client.shutdown().await.unwrap();

        assert_eq!(read_all(&b1).await, b"hello");
        assert_eq!(read_all(&b2).await, b"hello");

        let point = wait_for_data_point(&prober).await;
        assert_eq!(point.bytes_sent, 5);
        assert!(point.error.is_none());

        let summary = &prober.summary()[0];
        assert_eq!(summary.total_bytes_sent, 5);
        assert_eq!(summary.total_connections, 1);
        assert_eq!(summary.active_connections, 0);

        proxy.stop().await;
    }

    #[tokio::test]
    async fn test_skips_unreachable_egress() {
        let (good, good_addr) = backend().await;
        let (dead, dead_addr) = backend().await;
        drop(dead);
        let (proxy, _) = started(OperatingMode::Proxy, vec![dead_addr, good_addr]).await;

        let mut client = TcpStream::connect(proxy.local_addrs()[0]).await.unwrap();
        client.write_all(b"still here").await.unwrap();
        client.shutdown().await.unwrap();

        assert_eq!(read_all(&good).await, b"still here");
        proxy.stop().await;
    }

    #[tokio::test]
    async fn test_records_error_when_no_egress_reachable() {
        let (dead, dead_addr) = backend().await;
        drop(dead);
        let (proxy, prober) = started(OperatingMode::Proxy, vec![dead_addr]).await;

        let _client = TcpStream::connect(proxy.local_addrs()[0]).await.unwrap();
        let point = wait_for_data_point(&prober).await;
        assert!(point.error.unwrap().contains("none of the egress"));
        proxy.stop().await;
    }

    #[tokio::test]
    async fn test_reverse_proxy_relays_replies() {
        let (echo, echo_addr) = backend().await;
        tokio::spawn(async move {
            let (mut stream, _) = echo.accept().await.unwrap();
            let (mut rd, mut wr) = stream.split();
            tokio::io::copy(&mut rd, &mut wr).await.unwrap();
        });
        let (proxy, _) = started(OperatingMode::ReverseProxy, vec![echo_addr]).await;

        let mut client = TcpStream::connect(proxy.local_addrs()[0]).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        client.shutdown().await.unwrap();

        let mut reply = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut reply))
            .await
            .expect("reply timed out")
            .unwrap();
        assert_eq!(reply, b"ping");
        proxy.stop().await;
    }

    #[tokio::test]
    async fn test_stop_closes_idle_connections() {
        let (backend, backend_addr) = backend().await;
        let (proxy, prober) = started(OperatingMode::Proxy, vec![backend_addr]).await;
        let addr = proxy.local_addrs()[0];

        let _client = TcpStream::connect(addr).await.unwrap();
        let _ = backend.accept().await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), proxy.stop())
            .await
            .expect("stop hung on an idle connection");

        assert_eq!(prober.summary()[0].active_connections, 0);
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn test_start_fails_when_address_in_use() {
        let (_taken, taken_addr) = backend().await;
        let prober = Arc::new(HealthProber::new());
        let mut inst = instance(OperatingMode::Proxy, vec![taken_addr]);
        inst.ingress_addresses = vec![taken_addr];
        let mut proxy = Proxy::build(&inst, prober);
        assert!(proxy.start().await.is_err());
    }
}
