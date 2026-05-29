use tracing::info;

pub fn init_tracing(service_name: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_target(false)
        .init();
    info!("Tracing initialized for service: {}", service_name);
}

pub fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

pub mod error {
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum HivemindError {
        #[error("authentication failed: {0}")]
        AuthError(String),
        #[error("resource not found: {0}")]
        NotFound(String),
        #[error("invalid input: {0}")]
        InvalidInput(String),
        #[error("database error: {0}")]
        DatabaseError(String),
        #[error("redis error: {0}")]
        RedisError(String),
        #[error("gRPC error: {0}")]
        GrpcError(String),
        #[error("internal error: {0}")]
        InternalError(String),
    }

    pub type HivemindResult<T> = Result<T, HivemindError>;
}
