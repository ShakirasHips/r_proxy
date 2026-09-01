mod app;
use anyhow::Result;
use crate::app::App;



#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::default();
    app.start().await;
    app.run().await;

    Ok(())
}