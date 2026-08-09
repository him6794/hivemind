#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if hivemind_bin::is_managed_proof_verifier_mode(&args) {
        hivemind_bin::run_managed_proof_verifier();
    }
    hivemind_bin::run_service(hivemind_bin::ServiceRole::Nodepool).await
}
