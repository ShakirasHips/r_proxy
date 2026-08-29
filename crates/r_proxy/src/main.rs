use proxy::Server;
use anyhow::Result;

fn main() -> Result<()> {
    let mut server = Server::build("./test.toml")?;
    server.start()?;
    server.run()?;
    server.stop();
    Ok(())
}