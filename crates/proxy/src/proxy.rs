use crate::config::Instance;
use crate::health_prober::{HealthDataPoint, HealthProber};
use anyhow::{Context, Result};
use chrono::Utc;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use std::{io, thread};

struct ProxyContext {
    name: String,
    egress_addresses: Vec<SocketAddr>,
    ttl: u64,
    shutdown_flag: AtomicBool,
    health_prober: Arc<HealthProber>,
}

pub struct Proxy {
    settings: Instance,
    ingress_handles: Vec<JoinHandle<()>>,
    egress_handles: Vec<JoinHandle<()>>,
    ctx: Arc<ProxyContext>,
}

impl Proxy {
    pub fn build(instance: &Instance, health_prober: Arc<HealthProber>) -> Proxy {
        Proxy {
            settings: instance.clone(),
            ingress_handles: Vec::new(),
            egress_handles: Vec::new(),
            ctx: Arc::new(ProxyContext {
                name: instance.name.clone(),
                egress_addresses: instance.egress_addresses.clone(),
                ttl: instance.ttl,
                shutdown_flag: AtomicBool::new(false),
                health_prober,
            }),
        }
    }

    pub fn start(&mut self) -> Result<()> {
        for ingress in &self.settings.ingress_addresses {
            let ctx = self.ctx.clone();
            let handle = Self::spawn_ingress_listener(*ingress, ctx);
            self.egress_handles.push(handle);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.ctx.shutdown_flag.store(true, Ordering::SeqCst);

        for listen in self.settings.ingress_addresses.iter() {
            let _ = TcpStream::connect(listen);
        }

        for thread in self.egress_handles.drain(..) {
            if let Err(e) = thread.join() {
                log::error!("Thread join error: {:?}", e);
            }
        }

        for thread in self.ingress_handles.drain(..) {
            if let Err(e) = thread.join() {
                log::error!("Thread join error: {:?}", e);
            }
        }

        Ok(())
    }

    fn spawn_ingress_listener(ingress: SocketAddr, ctx: Arc<ProxyContext>) -> JoinHandle<()> {
        thread::spawn(move || {
            let listener = match TcpListener::bind(ingress)
                .with_context(|| format!("failed to bind listener on {ingress}"))
            {
                Ok(l) => l,
                Err(e) => {
                    log::error!("{e:#}");
                    return;
                }
            };
            Self::accept_loop(listener, ingress, ctx);
        })
    }

    fn accept_loop(listener: TcpListener, ingress: SocketAddr, ctx: Arc<ProxyContext>) {
        for stream in listener.incoming() {
            if ctx.shutdown_flag.load(Ordering::Relaxed) {
                break;
            }
            match stream {
                Ok(s) => {
                    Self::spawn_connection_handler(s, ingress, ctx.clone());
                }
                Err(e) => log::error!("incoming stream error: {e}"),
            }
        }
    }

    fn spawn_connection_handler(
        stream: TcpStream,
        ingress: SocketAddr,
        ctx: Arc<ProxyContext>,
    ) -> JoinHandle<()> {
        thread::spawn(move || {
            let out_streams = match Self::connect_egress(&ctx.egress_addresses, ctx.ttl) {
                Ok(streams) => streams,
                Err(e) => {
                    log::error!("egress connect error: {e:#}");
                    return;
                }
            };
            if let Err(e) = Self::handle(&stream, ingress, &out_streams, ctx) {
                log::error!("handle error: {e:#}");
            }
        })
    }

    fn connect_egress(egress_addresses: &[SocketAddr], ttl: u64) -> Result<Vec<TcpStream>> {
        egress_addresses
            .iter()
            .map(|addr| {
                let stream = TcpStream::connect_timeout(addr, Duration::from_secs(ttl))
                    .with_context(|| format!("failed to connect to {addr}"))?;

                stream
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .with_context(|| format!("failed to set read timeout for {addr}"))?;

                Ok(stream)
            })
            .collect()
    }

    fn relay_forward(
        mut in_stream: TcpStream,
        mut out_streams: Vec<TcpStream>,
        ingress: SocketAddr,
        ctx: Arc<ProxyContext>,
    ) -> Result<()> {
        let mut buf = [0u8; 4096];
        let mut bytes_sent: u32 = 0;

        loop {
            let n = in_stream
                .read(&mut buf)
                .context("failed reading from in_stream")?;
            if n == 0 {
                break;
            }
            for ts in out_streams.iter_mut() {
                ts.write_all(&buf[..n])
                    .context("failed writing to out_stream")?;
            }
            bytes_sent = bytes_sent.saturating_add(n as u32);

            if ctx.shutdown_flag.load(Ordering::Relaxed) {
                out_streams.push(in_stream);

                let _ = out_streams
                    .drain(..)
                    .map(|x| x.shutdown(Shutdown::Both))
                    .collect::<io::Result<Vec<_>>>()
                    .context("Failed to shut down streams properly");

                break;
            }
        }

        ctx.health_prober.insert(HealthDataPoint {
            id: ctx.name.clone(),
            ingress_addresses: vec![ingress.to_string()],
            egress_addresses: ctx.egress_addresses.iter().map(|a| a.to_string()).collect(),
            bytes_sent,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    fn handle(
        in_stream: &TcpStream,
        ingress: SocketAddr,
        target_streams: &[TcpStream],
        ctx: Arc<ProxyContext>,
    ) -> Result<()> {
        let in_stream_c = in_stream.try_clone().context("failed to clone in_stream")?;
        let out_streams = target_streams
            .iter()
            .map(|x| x.try_clone())
            .collect::<io::Result<Vec<_>>>()
            .context("failed to clone target_stream")?;

        Self::relay_forward(in_stream_c, out_streams, ingress, ctx)
    }

    fn handle_bidirectional(
        in_stream: &TcpStream,
        ingress: SocketAddr,
        target_streams: &[TcpStream],
        ctx: Arc<ProxyContext>,
    ) -> Result<()> {
        let out_streams = target_streams
            .iter()
            .map(|x| x.try_clone())
            .collect::<io::Result<Vec<_>>>()
            .context("failed to clone target_stream")?;

        let mut reverse_handles = vec![];
        for mut rev in out_streams
            .iter()
            .map(|s| s.try_clone())
            .collect::<io::Result<Vec<_>>>()
            .context("failed to clone out_stream for reverse relay")?
        {
            let mut in_stream_c = in_stream.try_clone().context("failed to clone in_stream")?;
            let ctx_c = ctx.clone();
            reverse_handles.push(thread::spawn(move || -> Result<()> {
                let mut buf = [0u8; 4096];
                loop {
                    let n = rev
                        .read(&mut buf)
                        .context("failed reading from target stream")?;
                    if n == 0 {
                        break;
                    }
                    in_stream_c
                        .write_all(&buf[..n])
                        .context("failed writing back to in_stream")?;

                    if ctx_c.shutdown_flag.load(Ordering::Relaxed) {
                        break;
                    }
                }
                Ok(())
            }));
        }

        let in_stream_c = in_stream.try_clone().context("failed to clone in_stream")?;
        let forward_result = Self::relay_forward(in_stream_c, out_streams, ingress, ctx);

        for thread in reverse_handles {
            thread.join().unwrap()?;
        }

        forward_result
    }
}
