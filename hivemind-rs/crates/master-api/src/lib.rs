pub mod grpc_client;
pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod vpn_bootstrap;

#[cfg(test)]
mod integration_tests;

use anyhow::Result;
use axum::Router;
use hivemind_client_runtime as client_runtime;
use hivemind_config::HivemindConfig;
use std::sync::Arc;
use tower_http::services::ServeDir;

use crate::grpc_client::GrpcClient;

pub struct MasterApiServer {
    app: Router,
}

impl MasterApiServer {
    pub async fn new(nodepool_grpc_addr: String, config: HivemindConfig) -> Result<Self> {
        // Optional operator-provisioned VPN auth key bootstrap. Downloaded masters
        // typically skip this and auto-issue a preauth key via website-api on login.
        crate::vpn_bootstrap::ensure_master_vpn(&config).await?;

        // Do not block UI startup on nodepool reachability. Remote masters often
        // need login-driven VPN bootstrap before the overlay path exists.
        let grpc = GrpcClient::new(nodepool_grpc_addr);
        let state = handlers::AppState {
            grpc_client: grpc,
            config,
            task_submit_limiter: Arc::new(tokio::sync::Mutex::new(
                handlers::TaskSubmitRateLimiter::new(),
            )),
        };
        let app = routes::create_router(state);
        Ok(Self { app })
    }

    pub async fn serve(self, addr: &str) -> Result<()> {
        self.serve_with_ui(addr, "./frontend/master-ui/dist").await
    }

    pub async fn serve_with_ui(self, addr: &str, ui_dir: &str) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Master API server listening on {}", addr);
        tracing::info!("Master UI directory: {}", ui_dir);
        let open_addr = addr.to_string();
        tokio::spawn(async move {
            client_runtime::open_ui_when_ready(&open_addr).await;
        });
        let app = if std::path::Path::new(ui_dir).is_dir() {
            self.app
                .fallback_service(ServeDir::new(ui_dir).append_index_html_on_directories(true))
        } else {
            self.app
        };
        axum::serve(listener, app)
            .await
            .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;
        Ok(())
    }
}
