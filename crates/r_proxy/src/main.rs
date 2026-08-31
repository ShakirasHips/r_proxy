use proxy::Server;
use ui::UI;
use outpost::Outpost;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut server = Server::build("./test.toml")?;
    server.start()?;
    server.run().await;

    Ok(())
}