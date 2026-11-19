//! Runs catalog-pinned llama.cpp packages and GGUF models on one host.
//!
//! The crate checks the runtime package against its manifest, checks the model
//! file against its catalog digest, computes a safe GPU offload (`-ngl`) and
//! context/KV-cache plan from host RAM/VRAM, starts a loopback-only server with
//! a permission-restricted per-process key file (`--api-key-file`), and returns
//! either unary or streamed token chunks and billing counts. Production
//! providers must run under `IsolationPolicy::Required` / `RequiredOci` (Linux
//! rootless OCI container with read-only rootfs, deny-all network, cgroups, and
//! seccomp; non-Linux hosts fail closed).

pub mod isolation;
pub mod manifest;
pub mod server;

pub use isolation::{
    build_oci_spec, IsolationPolicy, OciSandboxConfig, SandboxMounts, CONTAINER_API_KEY_FILE,
    CONTAINER_MODEL_FILE, CONTAINER_RUNTIME_DIR, DEFAULT_SANDBOX_CPU_PERIOD_US,
    DEFAULT_SANDBOX_MEMORY_LIMIT_BYTES, DEFAULT_SANDBOX_PIDS_LIMIT,
};
pub use manifest::{
    generate as generate_manifest, parse_pinned, sha256_file, ManifestError, RuntimeManifest,
};
pub use server::{
    build_server_args, compute_safe_offload_plan, parse_sse_data_line, ApiKeyFile,
    CompletionRequest, CompletionResponse, GpuBackend, GpuOffloadPolicy, HardwareMemoryBudget,
    LlamaServer, OffloadPlan, RuntimeError, ServerConfig, StreamChunk, MAX_NEW_TOKENS,
    MAX_PROMPT_BYTES, MIN_SAFE_CONTEXT,
};
