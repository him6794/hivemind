pub mod routes;
pub mod handlers;
pub mod middleware;

use anyhow::Result;
use axum::Router;
use hivemind_auth::AuthManager;
use hivemind_database::DatabaseManager;
use hivemind_task_scheduler::TaskScheduler;

pub struct MasterApiServer {
    app: Router,
}

impl MasterApiServer {
    pub fn new(
        db: DatabaseManager,
        auth: AuthManager,
        scheduler: TaskScheduler,
        nodepool_grpc_addr: String,
    ) -> Self {
        let state = handlers::AppState {
            db,
            auth,
            scheduler,
            nodepool_grpc_addr,
        };

        let app = routes::create_router(state);
        Self { app }
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration_tests;
