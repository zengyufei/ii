use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ii::run_cli().await
}
