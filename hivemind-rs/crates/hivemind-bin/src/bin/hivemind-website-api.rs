#[tokio::main]
async fn main() -> anyhow::Result<()> {
    hivemind_bin::run_service(hivemind_bin::ServiceRole::Website).await
}
