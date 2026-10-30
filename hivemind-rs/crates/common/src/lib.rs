use tracing::info;

pub fn init_tracing(service_name: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    info!("Tracing initialized for service: {}", service_name);
}

/// Where a windowed build writes its log: `HIVEMIND_LOG_DIR` when set, otherwise
/// `%LOCALAPPDATA%\Hivemind\logs`.
pub fn gui_log_dir() -> std::path::PathBuf {
    std::env::var_os("HIVEMIND_LOG_DIR")
        .filter(|dir| !dir.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(|base| std::path::PathBuf::from(base).join("Hivemind").join("logs"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("Hivemind").join("logs"))
}

/// A windowed build has no console, so its stdout and stderr would go nowhere. Point both at
/// `<log dir>/<name>.log` instead, moving a file that has grown past 10 MiB to `<name>.log.1`.
/// Call this before anything writes to stdout or stderr.
#[cfg(windows)]
pub fn redirect_std_streams_to_log_file(name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::io::AsRawHandle;

    const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const STD_ERROR_HANDLE: u32 = (-12i32) as u32;

    unsafe extern "system" {
        fn SetStdHandle(std_handle: u32, handle: *mut std::ffi::c_void) -> i32;
    }

    let dir = gui_log_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.log"));
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES) {
        let _ = std::fs::rename(&path, dir.join(format!("{name}.log.1")));
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    // SAFETY: the file is leaked below, so its handle stays valid for the life of the process.
    let stdout_ok = unsafe { SetStdHandle(STD_OUTPUT_HANDLE, file.as_raw_handle()) };
    let stderr_ok = unsafe { SetStdHandle(STD_ERROR_HANDLE, file.as_raw_handle()) };
    std::mem::forget(file);
    if stdout_ok == 0 || stderr_ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(path)
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
