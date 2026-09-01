use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use outpost::Outpost;
use proxy::Server;

#[derive(Default)]
pub struct App {
    outpost_handle: Option<tokio::task::JoinHandle<()>>,
    proxy_handle : Option<tokio::task::JoinHandle<()>>
}

impl App {
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let mut server = Server::build("./test.toml").unwrap();
        let prober = server.health_prober.clone();
        self.proxy_handle = Some(tokio::spawn(async move{
            server.start();
        }));

        self.outpost_handle = Some(tokio::spawn(async {
            Outpost::start(prober).await
        }));

        Ok(())
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        if let Some(handle) = self.outpost_handle.take() {
            handle.await?;
        }
        if let Some(handle) = self.proxy_handle.take() {
            handle.await?;
        }

        Ok(())
    }
}