mod app;
use anyhow::Result;
use crate::app::App;

const DEFAULT_CONFIG_PATH: &str = "test.toml";

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_CONFIG_PATH.to_string());

    let result = App::new(config_path).run().await;
    ui::UI::flush_logs();
    result
}
