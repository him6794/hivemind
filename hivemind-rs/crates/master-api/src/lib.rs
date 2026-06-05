pub mod grpc_client;
pub mod handlers;
pub mod middleware;
pub mod routes;

#[cfg(test)]
mod integration_tests;

use anyhow::Result;
use axum::Router;
use hivemind_config::HivemindConfig;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::grpc_client::GrpcClient;

pub struct MasterApiServer {
    app: Router,
}

impl MasterApiServer {
    pub async fn new(
        jwt_secret: String,
        token_expiry_hours: i64,
        nodepool_grpc_addr: String,
        config: HivemindConfig,
    ) -> Result<Self> {
        let grpc = GrpcClient::connect(&nodepool_grpc_addr)
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
            grpc_client: Arc::new(Mutex::new(grpc)),
            config,
            task_submit_limiter: Arc::new(Mutex::new(handlers::TaskSubmitRateLimiter::new())),
        };
        let app = routes::create_router(state);
        Ok(Self { app })
    }

    pub async fn serve(self, addr: &str) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Master API server listening on {}", addr);
        axum::serve(listener, self.app)
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
        Ok(())
    }
}
