pub mod grpc_client;
pub mod handlers;
pub mod middleware;
pub mod routes;

use anyhow::Result;
use axum::Router;
use hivemind_config::HivemindConfig;
use tokio::time::Duration;

use crate::grpc_client::GrpcClient;

/// Public website HTTP API.
///
/// This service is the official product entrypoint for account, CPT, and VPN
/// provisioning. It is intentionally separate from user-deployed master HTTP
/// APIs used by master-ui / requestor clients.
pub struct WebsiteApiServer {
    app: Router,
}

impl WebsiteApiServer {
    pub async fn new(
        jwt_secret: String,
        token_expiry_hours: i64,
        nodepool_grpc_addr: String,
        config: HivemindConfig,
    ) -> Result<Self> {
        let grpc =
            GrpcClient::connect_with_retry(&nodepool_grpc_addr, 50, Duration::from_millis(200))
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to connect to nodepool gRPC at {}: {}",
                        nodepool_grpc_addr,
                        e
                    )
                })?;
        let state = handlers::AppState {
            jwt_secret,
            token_expiry_hours,
            grpc_client: grpc,
            config,
        };
        let app = routes::create_router(state);
        Ok(Self { app })
    }

    pub async fn serve(self, addr: &str) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Website API server listening on {}", addr);
        axum::serve(listener, self.app)
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
        Ok(())
    }
}
