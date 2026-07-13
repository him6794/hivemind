#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hivemind_bin::run_from_cli(std::env::args().collect()).await
}
