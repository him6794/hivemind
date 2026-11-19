//! Model server process: one llama-server per model, bound to loopback.
//!
//! The server listens only on 127.0.0.1 and requires a random per-process key.
//! The key is written to a permission-restricted temporary key file passed via
//! `--api-key-file` (never via `LLAMA_API_KEY` environment variables or CLI
//! arguments) and is wiped and removed when the server drops. Before the first
//! request, the server must refuse an unauthenticated call; otherwise startup
//! fails closed.
//!
//! Prompt caching is off, so `tokens_evaluated` reports every prompt token.
//! Both unary (`complete`) and SSE token-streaming (`complete_stream`) paths
//! are supported, along with hardware-aware GPU layer offload (`-ngl`) and
//! safe RAM/VRAM + KV-cache planning.

use std::collections::VecDeque;
use std::io::Write;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

use crate::isolation::{
    build_oci_spec, verify_no_symlink, IsolationPolicy, OciSandboxConfig, SandboxMounts,
    CONTAINER_API_KEY_FILE, CONTAINER_MODEL_FILE, CONTAINER_RUNTIME_DIR,
};

/// Largest prompt accepted, in UTF-8 bytes.
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// Largest output a single request may ask for.
pub const MAX_NEW_TOKENS: u32 = 4096;
/// Largest response body read from the server.
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;
/// Minimum safe context size when down-sizing for memory pressure.
pub const MIN_SAFE_CONTEXT: u32 = 512;
const MAX_CONTEXT: u32 = 131_072;
const MAX_THREADS: u32 = 256;
const LOG_TAIL_BYTES: usize = 16 * 1024;
const POLL: Duration = Duration::from_millis(100);
const RAM_HEADROOM_FLOOR_BYTES: u64 = 256 * 1024 * 1024;
const VRAM_HEADROOM_FLOOR_BYTES: u64 = 256 * 1024 * 1024;

/// Target compute backend for `llama-server` layer offloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuBackend {
    #[default]
    Auto,
    Cpu,
    Cuda,
    Metal,
    Vulkan,
    Rocm,
}

impl GpuBackend {
    /// Whether this backend can offload transformer layers to device VRAM/unified memory.
    pub fn supports_offload(self) -> bool {
        !matches!(self, Self::Cpu)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Metal => "metal",
            Self::Vulkan => "vulkan",
            Self::Rocm => "rocm",
        }
    }
}

/// How `-ngl` (number of GPU layers) is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuOffloadPolicy {
    /// Compute safe `-ngl` automatically from `HardwareMemoryBudget` (or 0 if CPU-only).
    #[default]
    Auto,
    /// Offload an exact number of layers (`0` forces pure CPU).
    Fixed(u32),
    /// Offload all layers (`999` sentinel understood by `llama-server`).
    All,
}

/// Host RAM / VRAM snapshot used to calculate a safe `-ngl` and context plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareMemoryBudget {
    pub available_ram_bytes: u64,
    pub available_vram_bytes: u64,
    pub backend: GpuBackend,
}

/// Safe resource allocation computed for loading a GGUF model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffloadPlan {
    pub backend: GpuBackend,
    pub gpu_layers: u32,
    pub effective_context_size: u32,
    pub estimated_kv_cache_bytes: u64,
    pub estimated_vram_bytes: u64,
    pub estimated_ram_bytes: u64,
}

/// Computes a safe `-ngl` layer count and context/KV-cache cap from model size,
/// layer count, and available host RAM/VRAM.
///
/// Reserves safety headroom (15% or at least 256 MiB for RAM; 10% or at least
/// 256 MiB for VRAM when a GPU backend is active) so the host does not OOM.
pub fn compute_safe_offload_plan(
    model_bytes: u64,
    total_layers: u32,
    requested_context: u32,
    max_catalog_context: u32,
    budget: HardwareMemoryBudget,
) -> Result<OffloadPlan, RuntimeError> {
    if model_bytes == 0 {
        return Err(RuntimeError::InvalidConfig("model_bytes must be positive"));
    }
    if requested_context == 0 || requested_context > MAX_CONTEXT {
        return Err(RuntimeError::InvalidConfig(
            "context size must be 1 to 131072",
        ));
    }
    if max_catalog_context == 0 || requested_context > max_catalog_context {
        return Err(RuntimeError::InvalidConfig(
            "context size exceeds the model catalog limit",
        ));
    }

    let layers = total_layers.max(1);
    let usable_ram = usable_memory(budget.available_ram_bytes, 15, RAM_HEADROOM_FLOOR_BYTES);
    let usable_vram = if budget.backend.supports_offload() && budget.available_vram_bytes > 0 {
        usable_memory(budget.available_vram_bytes, 10, VRAM_HEADROOM_FLOOR_BYTES)
    } else {
        0
    };

    // Conservative KV-cache footprint per token scaled by model size (16 KiB..512 KiB/token).
    let kv_bytes_per_token = (model_bytes / 32_768).clamp(16 * 1024, 512 * 1024);
    // Compute buffer + runtime working set overhead (~12% of model weights, min 64 MiB).
    let runtime_overhead = (model_bytes / 8).max(64 * 1024 * 1024);

    // Pick the largest safe context <= requested_context (halving down to MIN_SAFE_CONTEXT).
    let total_usable = usable_ram.saturating_add(usable_vram);
    let mut effective_context = requested_context;
    loop {
        let kv_bytes = kv_bytes_per_token.saturating_mul(u64::from(effective_context));
        let needed_total = model_bytes
            .saturating_add(kv_bytes)
            .saturating_add(runtime_overhead);
        if needed_total <= total_usable {
            break;
        }
        if effective_context <= MIN_SAFE_CONTEXT {
            return Err(RuntimeError::InsufficientMemory(format!(
                "model requires at least {} MiB (with {effective_context} ctx), but only {} MiB usable RAM+VRAM is available",
                needed_total.div_ceil(1024 * 1024),
                total_usable / (1024 * 1024),
            )));
        }
        effective_context = (effective_context / 2).max(MIN_SAFE_CONTEXT);
    }

    let kv_bytes = kv_bytes_per_token.saturating_mul(u64::from(effective_context));
    let bytes_per_layer = model_bytes.div_ceil(u64::from(layers));

    let (gpu_layers, vram_used, ram_used) = if usable_vram == 0 {
        let ram_total = model_bytes
            .saturating_add(kv_bytes)
            .saturating_add(runtime_overhead);
        if ram_total > usable_ram {
            return Err(RuntimeError::InsufficientMemory(format!(
                "CPU execution requires {} MiB RAM, only {} MiB usable",
                ram_total.div_ceil(1024 * 1024),
                usable_ram / (1024 * 1024),
            )));
        }
        (0u32, 0u64, ram_total)
    } else {
        // Reserve GPU compute buffer overhead (~64 MiB or half of runtime_overhead) in VRAM first.
        let gpu_overhead = (runtime_overhead / 2).min(usable_vram);
        let vram_for_weights_and_kv = usable_vram.saturating_sub(gpu_overhead);
        // If KV cache fits in VRAM alongside at least 1 layer, place KV cache in VRAM.
        let (kv_in_vram, vram_for_layers) =
            if vram_for_weights_and_kv >= kv_bytes.saturating_add(bytes_per_layer) {
                (true, vram_for_weights_and_kv - kv_bytes)
            } else {
                (false, vram_for_weights_and_kv)
            };
        let max_gpu_layers =
            (vram_for_layers / bytes_per_layer.max(1)).min(u64::from(layers)) as u32;
        let vram_weights = u64::from(max_gpu_layers)
            .saturating_mul(bytes_per_layer)
            .min(model_bytes);
        let ram_weights = model_bytes.saturating_sub(vram_weights);
        let vram_total = vram_weights
            .saturating_add(if kv_in_vram { kv_bytes } else { 0 })
            .saturating_add(if max_gpu_layers > 0 { gpu_overhead } else { 0 });
        let ram_total = ram_weights
            .saturating_add(if kv_in_vram { 0 } else { kv_bytes })
            .saturating_add(runtime_overhead);
        if ram_total > usable_ram {
            return Err(RuntimeError::InsufficientMemory(format!(
                "hybrid offload still requires {} MiB host RAM, only {} MiB usable",
                ram_total.div_ceil(1024 * 1024),
                usable_ram / (1024 * 1024),
            )));
        }
        (max_gpu_layers, vram_total, ram_total)
    };

    Ok(OffloadPlan {
        backend: if gpu_layers == 0 {
            GpuBackend::Cpu
        } else {
            budget.backend
        },
        gpu_layers,
        effective_context_size: effective_context,
        estimated_kv_cache_bytes: kv_bytes,
        estimated_vram_bytes: vram_used,
        estimated_ram_bytes: ram_used,
    })
}

fn usable_memory(total: u64, reserve_percent: u64, floor_bytes: u64) -> u64 {
    let pct_reserve = total.saturating_mul(reserve_percent) / 100;
    let reserve = pct_reserve.max(floor_bytes);
    total.saturating_sub(reserve)
}

/// How to start one model server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub context_size: u32,
    pub threads: u32,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
    /// Target hardware backend (CPU / CUDA / Metal / Vulkan / ROCm / Auto).
    pub gpu_backend: GpuBackend,
    /// Layer offload policy (`Auto`, `Fixed(n)`, or `All`).
    pub gpu_layers: GpuOffloadPolicy,
    /// Optional host RAM/VRAM budget used when `gpu_layers == GpuOffloadPolicy::Auto`.
    pub memory_budget: Option<HardwareMemoryBudget>,
    /// Optional GGUF byte size & transformer layer count for safe auto-offload calculation.
    pub model_bytes: Option<u64>,
    pub total_layers: Option<u32>,
    /// Optional catalog maximum context length to clamp auto-planned context.
    pub max_catalog_context: Option<u32>,
}

impl ServerConfig {
    /// Creates a standard `ServerConfig` with sensible CPU/auto defaults.
    pub fn new(binary: PathBuf, model: PathBuf, context_size: u32, threads: u32) -> Self {
        Self {
            binary,
            model,
            context_size,
            threads,
            startup_timeout: Duration::from_secs(120),
            request_timeout: Duration::from_secs(300),
            gpu_backend: GpuBackend::Auto,
            gpu_layers: GpuOffloadPolicy::Auto,
            memory_budget: None,
            model_bytes: None,
            total_layers: None,
            max_catalog_context: None,
        }
    }

    /// Resolves effective `(gpu_layers, effective_context_size)` before launching.
    pub fn resolve_launch_plan(&self) -> Result<(u32, u32), RuntimeError> {
        validate_config(self)?;
        match self.gpu_layers {
            GpuOffloadPolicy::Fixed(n) => {
                let effective = if self.gpu_backend == GpuBackend::Cpu {
                    0
                } else {
                    n
                };
                Ok((effective, self.context_size))
            }
            GpuOffloadPolicy::All => {
                let effective = if self.gpu_backend == GpuBackend::Cpu {
                    0
                } else {
                    999
                };
                Ok((effective, self.context_size))
            }
            GpuOffloadPolicy::Auto => {
                if let (Some(budget), Some(model_bytes)) = (self.memory_budget, self.model_bytes) {
                    let layers = self.total_layers.unwrap_or(32);
                    let max_ctx = self.max_catalog_context.unwrap_or(self.context_size);
                    let plan = compute_safe_offload_plan(
                        model_bytes,
                        layers,
                        self.context_size,
                        max_ctx,
                        budget,
                    )?;
                    Ok((plan.gpu_layers, plan.effective_context_size))
                } else if self.gpu_backend == GpuBackend::Cpu
                    || self.gpu_backend == GpuBackend::Auto
                {
                    Ok((0, self.context_size))
                } else {
                    // Explicit GPU backend without explicit budget: offload all layers.
                    Ok((999, self.context_size))
                }
            }
        }
    }
}

/// One completion request.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionRequest {
    pub prompt: String,
    pub max_new_tokens: u32,
    /// 0 is greedy and reproducible on one machine. The valid range is 0 to 2.
    pub temperature: f32,
    pub seed: u32,
}

/// The server's answer, with the counts the price is computed from.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponse {
    pub content: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub stop_type: String,
    pub prompt_per_second: f64,
    pub predicted_per_second: f64,
}

/// One incremental chunk emitted by `LlamaServer::complete_stream`.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamChunk {
    pub delta: String,
    pub stop: bool,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub stop_type: Option<String>,
    pub prompt_per_second: Option<f64>,
    pub predicted_per_second: Option<f64>,
}

/// Failures starting, using, or stopping a model server.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("isolation is not available: {0}")]
    IsolationUnavailable(&'static str),
    #[error("invalid configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("insufficient host memory or VRAM: {0}")]
    InsufficientMemory(String),
    #[error("invalid request: {0}")]
    InvalidRequest(&'static str),
    #[error("server failed to start: {0}")]
    Startup(String),
    #[error("server process failed: {0}")]
    Process(String),
    #[error("request to the server failed: {0}")]
    Transport(String),
    #[error("server answered with HTTP {0}")]
    Status(u16),
    #[error("server response does not have the expected shape: {0}")]
    Response(String),
    #[error("the server truncated the prompt; the request is not charged")]
    PromptTruncated,
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Deserialize)]
struct RawCompletion {
    content: String,
    tokens_evaluated: u64,
    tokens_predicted: u64,
    #[serde(default)]
    stop_type: String,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    timings: Option<RawTimings>,
}

#[derive(Deserialize)]
struct RawStreamChunk {
    #[serde(default)]
    content: String,
    #[serde(default)]
    stop: bool,
    #[serde(default)]
    tokens_evaluated: Option<u64>,
    #[serde(default)]
    tokens_predicted: Option<u64>,
    #[serde(default)]
    stop_type: Option<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    timings: Option<RawTimings>,
}

#[derive(Deserialize)]
struct RawTimings {
    #[serde(default)]
    prompt_per_second: f64,
    #[serde(default)]
    predicted_per_second: f64,
}

/// RAII temporary API key file with restricted permissions.
/// Wipes key bytes and deletes the file on Drop.
pub struct ApiKeyFile {
    path: PathBuf,
    len: usize,
}

impl ApiKeyFile {
    /// Creates a new uniquely named key file with owner-only permissions.
    pub fn create(api_key: &str) -> Result<Self, RuntimeError> {
        let mut suffix = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut suffix);
        let dir = std::env::temp_dir();
        let path = dir.join(format!("hivemind-llama-key-{}.key", hex::encode(suffix)));
        let payload = format!("{api_key}\n");

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
        }

        #[cfg(not(unix))]
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
        }

        Ok(Self {
            path,
            len: payload.len(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ApiKeyFile {
    fn drop(&mut self) {
        if let Ok(mut file) = std::fs::OpenOptions::new().write(true).open(&self.path) {
            let zeros = vec![0u8; self.len.max(64)];
            let _ = file.write_all(&zeros);
            let _ = file.sync_all();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Builds the CLI arguments passed to `llama-server`.
/// Uses `--api-key-file` instead of `LLAMA_API_KEY` env var and applies the resolved `-ngl`.
pub fn build_server_args(
    model_path: &Path,
    port: u16,
    context_size: u32,
    threads: u32,
    gpu_layers: u32,
    api_key_file: &Path,
) -> Vec<String> {
    vec![
        "-m".to_owned(),
        model_path.to_string_lossy().into_owned(),
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--port".to_owned(),
        port.to_string(),
        "-c".to_owned(),
        context_size.to_string(),
        "-t".to_owned(),
        threads.to_string(),
        "-ngl".to_owned(),
        gpu_layers.to_string(),
        "-np".to_owned(),
        "1".to_owned(),
        "--no-webui".to_owned(),
        "--no-slots".to_owned(),
        "--api-key-file".to_owned(),
        api_key_file.to_string_lossy().into_owned(),
    ]
}

/// A running model server. Dropping it kills the process and wipes the key file.
pub struct LlamaServer {
    child: Option<Child>,
    port: u16,
    base_url: String,
    api_key: String,
    _api_key_file: Option<ApiKeyFile>,
    _oci_bundle_dir: Option<tempfile::TempDir>,
    gpu_layers: u32,
    effective_context_size: u32,
    client: reqwest::Client,
    log_tail: Arc<Mutex<VecDeque<u8>>>,
}

impl LlamaServer {
    /// Starts the server and waits until it reports healthy.
    pub async fn start(
        config: &ServerConfig,
        policy: IsolationPolicy,
    ) -> Result<Self, RuntimeError> {
        let oci_config = policy.require_backend()?.cloned();
        let (gpu_layers, effective_context_size) = config.resolve_launch_plan()?;
        let port = free_loopback_port()?;
        let api_key = random_key();
        let api_key_file = ApiKeyFile::create(&api_key)?;

        let (mut command, oci_bundle_dir) = if let Some(sandbox) = oci_config.as_ref() {
            prepare_oci_command(
                sandbox,
                config,
                port,
                effective_context_size,
                gpu_layers,
                api_key_file.path(),
            )?
        } else {
            let mut cmd = Command::new(&config.binary);
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("LLAMA_") {
                    cmd.env_remove(name);
                }
            }
            let args = build_server_args(
                &config.model,
                port,
                effective_context_size,
                config.threads,
                gpu_layers,
                api_key_file.path(),
            );
            cmd.args(&args);
            (cmd, None)
        };

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|err| RuntimeError::Startup(err.to_string()))?;

        let log_tail = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_TAIL_BYTES)));
        if let Some(stdout) = child.stdout.take() {
            spawn_drain(stdout, Arc::clone(&log_tail));
        }
        if let Some(stderr) = child.stderr.take() {
            spawn_drain(stderr, Arc::clone(&log_tail));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(config.request_timeout)
            .build()
            .map_err(|err| RuntimeError::Startup(err.to_string()))?;

        let mut server = Self {
            child: Some(child),
            port,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
            _api_key_file: Some(api_key_file),
            _oci_bundle_dir: oci_bundle_dir,
            gpu_layers,
            effective_context_size,
            client,
            log_tail,
        };
        server.wait_ready(config.startup_timeout).await?;
        server.check_key_is_enforced().await?;
        Ok(server)
    }

    /// Connects a client wrapper to an already running loopback server (used in tests).
    #[cfg(test)]
    pub(crate) fn connect_existing_for_test(port: u16, api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("test client");
        Self {
            child: None,
            port,
            base_url: format!("http://127.0.0.1:{port}"),
            api_key,
            _api_key_file: None,
            _oci_bundle_dir: None,
            gpu_layers: 0,
            effective_context_size: 2048,
            client,
            log_tail: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Port the server listens on, on 127.0.0.1.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Resolved `-ngl` GPU layer count used to start this server.
    pub fn gpu_layers(&self) -> u32 {
        self.gpu_layers
    }

    /// Effective context window size used to start this server.
    pub fn effective_context_size(&self) -> u32 {
        self.effective_context_size
    }

    /// Runs one unary completion and returns its counts.
    pub async fn complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, RuntimeError> {
        validate_request(request)?;
        let body = serde_json::json!({
            "prompt": request.prompt,
            "n_predict": request.max_new_tokens,
            "temperature": request.temperature,
            "seed": request.seed,
            "cache_prompt": false,
            "stream": false,
        });
        let response = self
            .client
            .post(format!("{}/completion", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| RuntimeError::Transport(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(RuntimeError::Status(status.as_u16()));
        }
        let bytes = read_capped(response).await?;
        let raw: RawCompletion = serde_json::from_slice(&bytes)
            .map_err(|err| RuntimeError::Response(err.to_string()))?;
        if raw.truncated {
            return Err(RuntimeError::PromptTruncated);
        }
        if raw.tokens_predicted > u64::from(request.max_new_tokens) {
            return Err(RuntimeError::Response(
                "more tokens were generated than requested".to_owned(),
            ));
        }
        let timings = raw.timings.unwrap_or(RawTimings {
            prompt_per_second: 0.0,
            predicted_per_second: 0.0,
        });
        Ok(CompletionResponse {
            content: raw.content,
            prompt_tokens: raw.tokens_evaluated,
            completion_tokens: raw.tokens_predicted,
            stop_type: raw.stop_type,
            prompt_per_second: timings.prompt_per_second,
            predicted_per_second: timings.predicted_per_second,
        })
    }

    /// Runs a streaming completion (`stream: true`) and returns a channel of incremental SSE chunks.
    pub async fn complete_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<mpsc::Receiver<Result<StreamChunk, RuntimeError>>, RuntimeError> {
        validate_request(request)?;
        let body = serde_json::json!({
            "prompt": request.prompt,
            "n_predict": request.max_new_tokens,
            "temperature": request.temperature,
            "seed": request.seed,
            "cache_prompt": false,
            "stream": true,
        });
        let response = self
            .client
            .post(format!("{}/completion", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|err| RuntimeError::Transport(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(RuntimeError::Status(status.as_u16()));
        }

        let max_new = u64::from(request.max_new_tokens);
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            stream_sse_response(response, max_new, tx).await;
        });
        Ok(rx)
    }

    /// Stops the process and waits for it to exit.
    pub async fn shutdown(mut self) -> Result<(), RuntimeError> {
        if let Some(mut child) = self.child.take() {
            child.start_kill()?;
            child.wait().await?;
        }
        Ok(())
    }

    async fn wait_ready(&mut self, limit: Duration) -> Result<(), RuntimeError> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(child) = self.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    return Err(RuntimeError::Startup(format!(
                        "exited with {status}: {}",
                        self.log_text()
                    )));
                }
            }
            let healthy = match self
                .client
                .get(format!("{}/health", self.base_url))
                .send()
                .await
            {
                Ok(response) => response.status().is_success(),
                Err(_) => false,
            };
            if healthy {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(RuntimeError::Startup(format!(
                    "not ready after {limit:?}: {}",
                    self.log_text()
                )));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    async fn check_key_is_enforced(&self) -> Result<(), RuntimeError> {
        let response = self
            .client
            .post(format!("{}/completion", self.base_url))
            .json(&serde_json::json!({ "prompt": "x", "n_predict": 1 }))
            .send()
            .await
            .map_err(|err| RuntimeError::Transport(err.to_string()))?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            Ok(())
        } else {
            Err(RuntimeError::Startup(format!(
                "an unauthenticated request got HTTP {} instead of 401",
                response.status().as_u16()
            )))
        }
    }

    fn log_text(&self) -> String {
        let tail = self.log_tail.lock().unwrap_or_else(PoisonError::into_inner);
        let bytes: Vec<u8> = tail.iter().copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        let start = text.len().saturating_sub(2048);
        let start = (start..text.len())
            .find(|index| text.is_char_boundary(*index))
            .unwrap_or(text.len());
        text[start..].to_owned()
    }
}

fn prepare_oci_command(
    sandbox: &OciSandboxConfig,
    config: &ServerConfig,
    port: u16,
    context_size: u32,
    gpu_layers: u32,
    api_key_file: &Path,
) -> Result<(Command, Option<tempfile::TempDir>), RuntimeError> {
    let runtime_dir = config.binary.parent().ok_or(RuntimeError::InvalidConfig(
        "binary path has no parent directory",
    ))?;
    let binary_name = config
        .binary
        .file_name()
        .ok_or(RuntimeError::InvalidConfig("binary path has no file name"))?
        .to_string_lossy();
    verify_no_symlink(runtime_dir)?;
    verify_no_symlink(&config.model)?;
    verify_no_symlink(api_key_file)?;

    let mut container_args = vec![format!("{CONTAINER_RUNTIME_DIR}/{binary_name}")];
    container_args.extend(build_server_args(
        Path::new(CONTAINER_MODEL_FILE),
        port,
        context_size,
        config.threads,
        gpu_layers,
        Path::new(CONTAINER_API_KEY_FILE),
    ));

    let mounts = SandboxMounts {
        runtime_dir: runtime_dir.to_path_buf(),
        model_file: config.model.clone(),
        api_key_file: api_key_file.to_path_buf(),
    };
    let spec = build_oci_spec(sandbox, &mounts, &container_args)?;
    let bundle_dir = tempfile::tempdir()?;
    let spec_bytes =
        serde_json::to_vec_pretty(&spec).map_err(|err| RuntimeError::Startup(err.to_string()))?;
    std::fs::write(bundle_dir.path().join("config.json"), spec_bytes)?;

    let mut id_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    let container_id = format!("hm-llm-{}", hex::encode(id_bytes));

    let mut cmd = Command::new(&sandbox.runtime_binary);
    cmd.arg("run")
        .arg("--bundle")
        .arg(bundle_dir.path())
        .arg(&container_id);
    Ok((cmd, Some(bundle_dir)))
}

/// Parses one `data: ...` SSE line from `llama-server`.
pub fn parse_sse_data_line(
    line: &str,
    max_new_tokens: u64,
) -> Result<Option<StreamChunk>, RuntimeError> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(None);
    }
    let Some(data) = trimmed.strip_prefix("data:") else {
        return Ok(None);
    };
    let payload = data.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(None);
    }
    let raw: RawStreamChunk =
        serde_json::from_str(payload).map_err(|err| RuntimeError::Response(err.to_string()))?;
    if raw.truncated {
        return Err(RuntimeError::PromptTruncated);
    }
    if let Some(predicted) = raw.tokens_predicted {
        if predicted > max_new_tokens {
            return Err(RuntimeError::Response(
                "more tokens were generated than requested".to_owned(),
            ));
        }
    }
    Ok(Some(StreamChunk {
        delta: raw.content,
        stop: raw.stop,
        prompt_tokens: raw.tokens_evaluated,
        completion_tokens: raw.tokens_predicted,
        stop_type: raw.stop_type,
        prompt_per_second: raw.timings.as_ref().map(|t| t.prompt_per_second),
        predicted_per_second: raw.timings.as_ref().map(|t| t.predicted_per_second),
    }))
}

async fn stream_sse_response(
    mut response: reqwest::Response,
    max_new_tokens: u64,
    tx: mpsc::Sender<Result<StreamChunk, RuntimeError>>,
) {
    let mut total_bytes = 0usize;
    let mut buffer = String::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                total_bytes += chunk.len();
                if total_bytes > MAX_RESPONSE_BYTES {
                    let _ = tx
                        .send(Err(RuntimeError::Response(
                            "streaming response is over the size limit".to_owned(),
                        )))
                        .await;
                    return;
                }
                let text = match std::str::from_utf8(&chunk) {
                    Ok(valid) => valid,
                    Err(err) => {
                        let _ = tx.send(Err(RuntimeError::Response(err.to_string()))).await;
                        return;
                    }
                };
                buffer.push_str(text);
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].to_owned();
                    buffer.drain(..=newline_pos);
                    match parse_sse_data_line(&line, max_new_tokens) {
                        Ok(Some(parsed)) => {
                            let is_stop = parsed.stop;
                            if tx.send(Ok(parsed)).await.is_err() {
                                return;
                            }
                            if is_stop {
                                return;
                            }
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let _ = tx.send(Err(err)).await;
                            return;
                        }
                    }
                }
            }
            Ok(None) => {
                if !buffer.trim().is_empty() {
                    match parse_sse_data_line(&buffer, max_new_tokens) {
                        Ok(Some(parsed)) => {
                            let _ = tx.send(Ok(parsed)).await;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let _ = tx.send(Err(err)).await;
                        }
                    }
                }
                return;
            }
            Err(err) => {
                let _ = tx.send(Err(RuntimeError::Transport(err.to_string()))).await;
                return;
            }
        }
    }
}

fn validate_config(config: &ServerConfig) -> Result<(), RuntimeError> {
    if config.context_size == 0 || config.context_size > MAX_CONTEXT {
        return Err(RuntimeError::InvalidConfig(
            "context size must be 1 to 131072",
        ));
    }
    if config.threads == 0 || config.threads > MAX_THREADS {
        return Err(RuntimeError::InvalidConfig("threads must be 1 to 256"));
    }
    Ok(())
}

fn validate_request(request: &CompletionRequest) -> Result<(), RuntimeError> {
    if request.prompt.is_empty() {
        return Err(RuntimeError::InvalidRequest("prompt is empty"));
    }
    if request.prompt.len() > MAX_PROMPT_BYTES {
        return Err(RuntimeError::InvalidRequest("prompt is larger than 64 KiB"));
    }
    if request.max_new_tokens == 0 || request.max_new_tokens > MAX_NEW_TOKENS {
        return Err(RuntimeError::InvalidRequest(
            "max_new_tokens must be 1 to 4096",
        ));
    }
    if !request.temperature.is_finite() || !(0.0..=2.0).contains(&request.temperature) {
        return Err(RuntimeError::InvalidRequest(
            "temperature must be from 0 to 2",
        ));
    }
    Ok(())
}

fn free_loopback_port() -> Result<u16, RuntimeError> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

fn random_key() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, RuntimeError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| RuntimeError::Transport(err.to_string()))?
    {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(RuntimeError::Response(
                "response is over the size limit".to_owned(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn spawn_drain<R>(mut reader: R, tail: Arc<Mutex<VecDeque<u8>>>)
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let mut guard = tail.lock().unwrap_or_else(PoisonError::into_inner);
                    for byte in &buffer[..read] {
                        if guard.len() == LOG_TAIL_BYTES {
                            guard.pop_front();
                        }
                        guard.push_back(*byte);
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn requests_outside_the_documented_bounds_are_refused_before_sending() {
        let ok = CompletionRequest {
            prompt: "hello".to_owned(),
            max_new_tokens: 16,
            temperature: 0.0,
            seed: 1,
        };
        assert!(validate_request(&ok).is_ok());
        let empty = CompletionRequest {
            prompt: String::new(),
            ..ok.clone()
        };
        assert!(validate_request(&empty).is_err());
        let zero = CompletionRequest {
            max_new_tokens: 0,
            ..ok.clone()
        };
        assert!(validate_request(&zero).is_err());
        let hot = CompletionRequest {
            temperature: 2.5,
            ..ok.clone()
        };
        assert!(validate_request(&hot).is_err());
        let nan = CompletionRequest {
            temperature: f32::NAN,
            ..ok
        };
        assert!(validate_request(&nan).is_err());
    }

    #[test]
    fn keys_are_64_hex_characters_and_differ_between_processes() {
        let first = random_key();
        let second = random_key();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
    }

    #[test]
    fn key_file_is_created_with_key_and_deleted_on_drop() {
        let key = random_key();
        let path;
        {
            let kf = ApiKeyFile::create(&key).expect("create key file");
            path = kf.path().to_path_buf();
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).expect("read key file");
            assert_eq!(content.trim(), key);
        }
        assert!(!path.exists(), "key file must be removed on drop");
    }

    #[test]
    fn server_args_use_api_key_file_and_dynamic_gpu_layers() {
        let args = build_server_args(
            Path::new("/models/m.gguf"),
            18099,
            4096,
            8,
            24,
            Path::new("/run/key"),
        );
        let ngl_pos = args.iter().position(|a| a == "-ngl").expect("-ngl present");
        assert_eq!(args[ngl_pos + 1], "24");
        let kf_pos = args
            .iter()
            .position(|a| a == "--api-key-file")
            .expect("--api-key-file present");
        assert_eq!(args[kf_pos + 1], "/run/key");
        assert!(!args.iter().any(|a| a == "--api-key"));
    }

    #[test]
    fn offload_plan_computes_full_gpu_hybrid_and_cpu_safely() {
        let gib = 1024 * 1024 * 1024u64;
        // 4 GiB 32-layer model on 12 GiB VRAM + 16 GiB RAM -> all 32 layers on GPU
        let full = compute_safe_offload_plan(
            4 * gib,
            32,
            4096,
            8192,
            HardwareMemoryBudget {
                available_ram_bytes: 16 * gib,
                available_vram_bytes: 12 * gib,
                backend: GpuBackend::Cuda,
            },
        )
        .expect("full gpu plan");
        assert_eq!(full.gpu_layers, 32);
        assert_eq!(full.backend, GpuBackend::Cuda);
        assert_eq!(full.effective_context_size, 4096);

        // 4 GiB 32-layer model on 3 GiB VRAM + 16 GiB RAM -> hybrid partial offload
        let hybrid = compute_safe_offload_plan(
            4 * gib,
            32,
            4096,
            8192,
            HardwareMemoryBudget {
                available_ram_bytes: 16 * gib,
                available_vram_bytes: 3 * gib,
                backend: GpuBackend::Vulkan,
            },
        )
        .expect("hybrid plan");
        assert!(hybrid.gpu_layers > 0 && hybrid.gpu_layers < 32);
        assert_eq!(hybrid.backend, GpuBackend::Vulkan);

        // CPU-only backend -> 0 GPU layers
        let cpu = compute_safe_offload_plan(
            4 * gib,
            32,
            4096,
            8192,
            HardwareMemoryBudget {
                available_ram_bytes: 16 * gib,
                available_vram_bytes: 12 * gib,
                backend: GpuBackend::Cpu,
            },
        )
        .expect("cpu plan");
        assert_eq!(cpu.gpu_layers, 0);
        assert_eq!(cpu.backend, GpuBackend::Cpu);

        // Too little total memory -> InsufficientMemory error
        let oom = compute_safe_offload_plan(
            8 * gib,
            32,
            4096,
            8192,
            HardwareMemoryBudget {
                available_ram_bytes: 2 * gib,
                available_vram_bytes: 0,
                backend: GpuBackend::Cpu,
            },
        );
        assert!(matches!(oom, Err(RuntimeError::InsufficientMemory(_))));
    }

    #[test]
    fn offload_plan_reduces_context_when_ram_is_tight() {
        let gib = 1024 * 1024 * 1024u64;
        // 4 GiB model with 32768 requested context (~4 GiB KV cache) on 7 GiB RAM -> shrinks context
        let plan = compute_safe_offload_plan(
            4 * gib,
            32,
            32_768,
            32_768,
            HardwareMemoryBudget {
                available_ram_bytes: 7 * gib,
                available_vram_bytes: 0,
                backend: GpuBackend::Cpu,
            },
        )
        .expect("tight plan shrinks context");
        assert!(plan.effective_context_size < 32_768);
        assert!(plan.effective_context_size >= MIN_SAFE_CONTEXT);
    }

    #[tokio::test]
    async fn complete_stream_yields_incremental_sse_tokens_and_final_counts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let expected_key = "test-stream-key".to_owned();
        let key_check = expected_key.clone();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut req_buf = vec![0u8; 4096];
            let read = socket.read(&mut req_buf).await.expect("read");
            let req_text = String::from_utf8_lossy(&req_buf[..read]);
            assert!(req_text.to_ascii_lowercase().contains(&format!(
                "authorization: bearer {}",
                key_check.to_ascii_lowercase()
            )));
            assert!(req_text.contains("\"stream\":true"));

            let sse_body = concat!(
                "data: {\"content\":\"Paris\",\"stop\":false}\n\n",
                "data: {\"content\":\" is\",\"stop\":false}\n\n",
                "data: {\"content\":\" nice.\",\"stop\":true,\"tokens_evaluated\":5,\"tokens_predicted\":3,\"stop_type\":\"eos\",\"timings\":{\"prompt_per_second\":120.0,\"predicted_per_second\":48.5}}\n\n",
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                sse_body.len(),
                sse_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        });

        let server = LlamaServer::connect_existing_for_test(port, expected_key);
        let mut rx = server
            .complete_stream(&CompletionRequest {
                prompt: "The capital of France is".to_owned(),
                max_new_tokens: 16,
                temperature: 0.0,
                seed: 1,
            })
            .await
            .expect("stream starts");

        let mut chunks = Vec::new();
        while let Some(item) = rx.recv().await {
            chunks.push(item.expect("valid chunk"));
        }
        assert_eq!(chunks.len(), 3);
        let combined: String = chunks.iter().map(|c| c.delta.as_str()).collect();
        assert_eq!(combined, "Paris is nice.");
        let last = chunks.last().expect("last");
        assert!(last.stop);
        assert_eq!(last.prompt_tokens, Some(5));
        assert_eq!(last.completion_tokens, Some(3));
        assert_eq!(last.stop_type.as_deref(), Some("eos"));
    }

    #[tokio::test]
    async fn a_required_policy_refuses_to_start_even_with_a_valid_binary() {
        let config = ServerConfig::new(
            PathBuf::from("does-not-exist"),
            PathBuf::from("does-not-exist.gguf"),
            512,
            1,
        );
        assert!(matches!(
            LlamaServer::start(&config, IsolationPolicy::Required).await,
            Err(RuntimeError::IsolationUnavailable(_))
        ));
    }
}
