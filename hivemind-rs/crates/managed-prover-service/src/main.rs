use anyhow::Result;
use hivemind_config::HivemindConfig;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    let config = HivemindConfig::load()?;
    hivemind_managed_prover_service::serve(config).await
}
