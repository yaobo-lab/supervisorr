#[tokio::main]
async fn main() -> anyhow::Result<()> {
    supervisord::cli().await
}
