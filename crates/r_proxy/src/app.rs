use anyhow::Context;
use outpost::Outpost;
use proxy::Server;
use tokio::sync::oneshot;

pub struct App {
    config_path: String,
}

impl App {
    pub fn new(config_path: String) -> App {
        App { config_path }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        ui::UI::init_logging();

        let mut server = Server::build(&self.config_path)?;
        let outpost = Outpost::bind(server.outpost_address())
            .await
            .with_context(|| format!("failed to bind outpost on {}", server.outpost_address()))?;
        server.start().await?;

        let prober = server.health_prober.clone();
        let (stop_outpost, outpost_stopped) = oneshot::channel::<()>();
        let outpost_handle = tokio::spawn(outpost.serve(prober.clone(), async {
            let _ = outpost_stopped.await;
        }));

        let ui_result = tokio::task::spawn_blocking(move || ui::UI::run(prober)).await;

        let _ = stop_outpost.send(());
        match outpost_handle.await {
            Ok(Err(e)) => log::error!("outpost error: {e}"),
            Err(e) => log::error!("outpost task failed: {e}"),
            Ok(Ok(())) => {}
        }
        server.shutdown().await?;

        ui_result.context("UI task failed")?.context("UI error")?;
        Ok(())
    }
}
