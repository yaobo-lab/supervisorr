#[tokio::main]
async fn main() -> anyhow::Result<()> {
    supervisorr::run_cli().await
}
