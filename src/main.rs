#[tokio::main]
async fn main() -> anyhow::Result<()> {
    supervisord::run_cli().await
}
