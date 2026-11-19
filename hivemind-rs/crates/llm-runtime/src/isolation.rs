//! Execution policy and formal rootless OCI sandbox backend for the model server.
//!
//! Production providers run under `IsolationPolicy::Required`:
//! - On Linux, `IsolationPolicy::Required` launches `llama-server` inside a
//!   rootless OCI container (`crun` / `runc`) with a read-only root filesystem,
//!   read-only bind mounts for the runtime directory, GGUF model, and key file,
//!   an isolated empty network namespace (no external network), cgroup v2
//!   memory/CPU/PID limits, `noNewPrivileges`, and a default-deny seccomp
//!   syscall allowlist.
//! - On non-Linux hosts (Windows / macOS), native rootless OCI container
//!   isolation is not provided and `IsolationPolicy::Required` fails closed.
//!   See `docs/PUBLIC_NETWORK_LIMITATIONS.md` and `AGENT.md`.
//!
//! `IsolationPolicy::UnsafeDevelopmentHost` runs directly on the host without
//! container isolation and is intended only for local development/testing on a
//! machine controlled by the operator.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::server::RuntimeError;

/// Default memory limit if an OCI sandbox config omits one (4 GiB).
pub const DEFAULT_SANDBOX_MEMORY_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Default PID limit inside the sandbox container.
pub const DEFAULT_SANDBOX_PIDS_LIMIT: i64 = 128;
/// Default CPU quota period in microseconds (100 ms).
pub const DEFAULT_SANDBOX_CPU_PERIOD_US: u64 = 100_000;

/// Configuration for the Linux rootless OCI container sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OciSandboxConfig {
    /// Path to the rootless OCI runtime binary (e.g., `/usr/bin/crun` or `/usr/bin/runc`).
    pub runtime_binary: PathBuf,
    /// Directory holding the read-only container root filesystem.
    pub rootfs_dir: PathBuf,
    /// Cgroup v2 hard memory ceiling in bytes.
    pub memory_limit_bytes: u64,
    /// Cgroup v2 CPU quota in microseconds per `cpu_period_us` (0 = 1 core).
    pub cpu_quota_us: i64,
    /// Cgroup v2 CPU period in microseconds.
    pub cpu_period_us: u64,
    /// Maximum number of processes/threads inside the container.
    pub pids_limit: i64,
    /// Host UID/GID mapped to container rootless user.
    pub host_uid: u32,
    pub host_gid: u32,
    /// Whether to allow loopback-only IPC (external network is ALWAYS denied).
    pub deny_external_network: bool,
}

impl OciSandboxConfig {
    /// Validates sandbox configuration invariants before building a spec.
    pub fn validate(&self) -> Result<(), RuntimeError> {
        if self.runtime_binary.as_os_str().is_empty() {
            return Err(RuntimeError::IsolationUnavailable(
                "OCI runtime binary path is empty",
            ));
        }
        if self.rootfs_dir.as_os_str().is_empty() {
            return Err(RuntimeError::IsolationUnavailable(
                "OCI rootfs directory path is empty",
            ));
        }
        if !self.deny_external_network {
            return Err(RuntimeError::IsolationUnavailable(
                "external network access cannot be enabled under IsolationPolicy::Required",
            ));
        }
        if self.memory_limit_bytes < 64 * 1024 * 1024 {
            return Err(RuntimeError::IsolationUnavailable(
                "OCI memory limit must be at least 64 MiB",
            ));
        }
        if self.pids_limit <= 0 || self.pids_limit > 4096 {
            return Err(RuntimeError::IsolationUnavailable(
                "OCI pids limit must be between 1 and 4096",
            ));
        }
        if self.cpu_period_us == 0 {
            return Err(RuntimeError::IsolationUnavailable(
                "OCI cpu_period_us must be positive",
            ));
        }
        Ok(())
    }
}

/// Bind mounts prepared for a sandboxed `llama-server` container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxMounts {
    pub runtime_dir: PathBuf,
    pub model_file: PathBuf,
    pub api_key_file: PathBuf,
}

/// Container-internal mount paths used inside the OCI spec.
pub const CONTAINER_RUNTIME_DIR: &str = "/opt/hivemind/runtime";
pub const CONTAINER_MODEL_FILE: &str = "/opt/hivemind/model/model.gguf";
pub const CONTAINER_API_KEY_FILE: &str = "/run/hivemind/api.key";

/// Builds a rootless OCI `config.json` specification enforcing:
/// - read-only rootfs
/// - read-only bind mounts for runtime, model, and API key file
/// - `noNewPrivileges` and dropped Linux capabilities
/// - isolated `network`, `pid`, `ipc`, `uts`, `mount`, `user` namespaces
/// - cgroup v2 memory, CPU, and PIDs hard limits
/// - default-deny seccomp policy with a minimal allowlist for `llama-server`
pub fn build_oci_spec(
    sandbox: &OciSandboxConfig,
    mounts: &SandboxMounts,
    container_args: &[String],
) -> Result<Value, RuntimeError> {
    sandbox.validate()?;
    if container_args.is_empty() {
        return Err(RuntimeError::InvalidConfig(
            "OCI container args must not be empty",
        ));
    }

    let cpu_quota = if sandbox.cpu_quota_us <= 0 {
        sandbox.cpu_period_us as i64
    } else {
        sandbox.cpu_quota_us
    };

    let allowed_syscalls: &[&str] = &[
        "accept",
        "accept4",
        "access",
        "arch_prctl",
        "bind",
        "brk",
        "clock_gettime",
        "clock_nanosleep",
        "clone",
        "clone3",
        "close",
        "epoll_create1",
        "epoll_ctl",
        "epoll_pwait",
        "epoll_wait",
        "eventfd2",
        "execve",
        "exit",
        "exit_group",
        "faccessat",
        "fcntl",
        "fstat",
        "futex",
        "getdents64",
        "getegid",
        "geteuid",
        "getgid",
        "getpid",
        "getrandom",
        "getrlimit",
        "getsockname",
        "getsockopt",
        "gettid",
        "getuid",
        "ioctl",
        "listen",
        "lseek",
        "madvise",
        "mmap",
        "mprotect",
        "munmap",
        "nanosleep",
        "newfstatat",
        "openat",
        "pipe2",
        "poll",
        "pread64",
        "prlimit64",
        "read",
        "recvfrom",
        "recvmsg",
        "rseq",
        "rt_sigaction",
        "rt_sigprocmask",
        "rt_sigreturn",
        "sched_getaffinity",
        "sched_yield",
        "sendmsg",
        "sendto",
        "set_robust_list",
        "set_tid_address",
        "setsockopt",
        "shutdown",
        "sigaltstack",
        "socket",
        "statx",
        "tgkill",
        "uname",
        "write",
        "writev",
    ];

    Ok(json!({
        "ociVersion": "1.0.2",
        "process": {
            "terminal": false,
            "user": { "uid": 0, "gid": 0 },
            "args": container_args,
            "env": [
                "PATH=/usr/local/bin:/usr/bin:/bin",
                "HOME=/tmp"
            ],
            "cwd": "/tmp",
            "capabilities": {
                "bounding": [],
                "effective": [],
                "inheritable": [],
                "permitted": [],
                "ambient": []
            },
            "rlimits": [
                { "type": "RLIMIT_NOFILE", "hard": 1024, "soft": 1024 }
            ],
            "noNewPrivileges": true
        },
        "root": {
            "path": sandbox.rootfs_dir.to_string_lossy(),
            "readonly": true
        },
        "hostname": "hivemind-llm-sandbox",
        "mounts": [
            {
                "destination": "/proc",
                "type": "proc",
                "source": "proc",
                "options": ["nosuid", "noexec", "nodev"]
            },
            {
                "destination": "/tmp",
                "type": "tmpfs",
                "source": "tmpfs",
                "options": ["nosuid", "noexec", "nodev", "size=64m"]
            },
            {
                "destination": CONTAINER_RUNTIME_DIR,
                "type": "bind",
                "source": mounts.runtime_dir.to_string_lossy(),
                "options": ["bind", "ro", "nosuid", "nodev"]
            },
            {
                "destination": CONTAINER_MODEL_FILE,
                "type": "bind",
                "source": mounts.model_file.to_string_lossy(),
                "options": ["bind", "ro", "nosuid", "noexec", "nodev"]
            },
            {
                "destination": CONTAINER_API_KEY_FILE,
                "type": "bind",
                "source": mounts.api_key_file.to_string_lossy(),
                "options": ["bind", "ro", "nosuid", "noexec", "nodev"]
            }
        ],
        "linux": {
            "uidMappings": [
                { "containerID": 0, "hostID": sandbox.host_uid, "size": 1 }
            ],
            "gidMappings": [
                { "containerID": 0, "hostID": sandbox.host_gid, "size": 1 }
            ],
            "namespaces": [
                { "type": "pid" },
                { "type": "network" },
                { "type": "ipc" },
                { "type": "uts" },
                { "type": "mount" },
                { "type": "user" }
            ],
            "resources": {
                "memory": {
                    "limit": sandbox.memory_limit_bytes,
                    "swap": sandbox.memory_limit_bytes
                },
                "cpu": {
                    "quota": cpu_quota,
                    "period": sandbox.cpu_period_us
                },
                "pids": {
                    "limit": sandbox.pids_limit
                }
            },
            "seccomp": {
                "defaultAction": "SCMP_ACT_ERRNO",
                "architectures": ["SCMP_ARCH_X86_64", "SCMP_ARCH_AARCH64"],
                "syscalls": [
                    {
                        "names": allowed_syscalls,
                        "action": "SCMP_ACT_ALLOW"
                    }
                ]
            }
        }
    }))
}

/// Checks whether a host path is safe to mount (exists and is not a symlink).
pub fn verify_no_symlink(path: &Path) -> Result<(), RuntimeError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| {
        RuntimeError::IsolationUnavailable("sandbox path does not exist or cannot be inspected")
    })?;
    if meta.file_type().is_symlink() {
        return Err(RuntimeError::IsolationUnavailable(
            "symlinks are refused as OCI sandbox mount sources",
        ));
    }
    Ok(())
}

/// How the model server may be executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationPolicy {
    /// Required for production providers.
    ///
    /// On Linux, requires a valid `OciSandboxConfig` pointing at an installed
    /// rootless OCI runtime (`crun`/`runc`) and rootfs. Without a sandbox
    /// config (or on non-Linux hosts), startup fails closed.
    Required,
    /// Required production isolation with an explicit rootless OCI sandbox config.
    RequiredOci(Box<OciSandboxConfig>),
    /// Runs the server directly on the host without a sandbox. Development and tests only.
    UnsafeDevelopmentHost,
}

impl IsolationPolicy {
    /// Fails unless the policy has an execution backend available on this host.
    pub fn require_backend(&self) -> Result<Option<&OciSandboxConfig>, RuntimeError> {
        match self {
            Self::Required => Err(RuntimeError::IsolationUnavailable(
                "IsolationPolicy::Required needs a configured Linux rootless OCI sandbox (use IsolationPolicy::RequiredOci on Linux)",
            )),
            Self::RequiredOci(config) => {
                config.validate()?;
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = config;
                    Err(RuntimeError::IsolationUnavailable(
                        "rootless OCI sandbox is only supported on Linux; non-Linux hosts cannot satisfy IsolationPolicy::Required",
                    ))
                }
                #[cfg(target_os = "linux")]
                {
                    verify_no_symlink(&config.runtime_binary)?;
                    verify_no_symlink(&config.rootfs_dir)?;
                    if !config.rootfs_dir.is_dir() {
                        return Err(RuntimeError::IsolationUnavailable(
                            "OCI rootfs path must be a directory",
                        ));
                    }
                    Ok(Some(config))
                }
            }
            Self::UnsafeDevelopmentHost => Ok(None),
        }
    }

    /// Short label for logs and output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Required | Self::RequiredOci(_) => "required",
            Self::UnsafeDevelopmentHost => "unsafe-development-host",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_oci_config(dir: &Path) -> OciSandboxConfig {
        let runtime = dir.join("crun");
        let rootfs = dir.join("rootfs");
        std::fs::write(&runtime, b"stub").expect("write runtime");
        std::fs::create_dir_all(&rootfs).expect("mkdir rootfs");
        OciSandboxConfig {
            runtime_binary: runtime,
            rootfs_dir: rootfs,
            memory_limit_bytes: DEFAULT_SANDBOX_MEMORY_LIMIT_BYTES,
            cpu_quota_us: 200_000,
            cpu_period_us: DEFAULT_SANDBOX_CPU_PERIOD_US,
            pids_limit: DEFAULT_SANDBOX_PIDS_LIMIT,
            host_uid: 1000,
            host_gid: 1000,
            deny_external_network: true,
        }
    }

    #[test]
    fn required_without_oci_config_fails_closed() {
        assert!(matches!(
            IsolationPolicy::Required.require_backend(),
            Err(RuntimeError::IsolationUnavailable(_))
        ));
    }

    #[test]
    fn required_oci_enforces_host_os_boundary() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = sample_oci_config(dir.path());
        let policy = IsolationPolicy::RequiredOci(Box::new(cfg));
        #[cfg(target_os = "linux")]
        assert!(policy.require_backend().is_ok());
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(
            policy.require_backend(),
            Err(RuntimeError::IsolationUnavailable(_))
        ));
    }

    #[test]
    fn oci_spec_enforces_readonly_root_no_network_cgroups_and_seccomp() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = sample_oci_config(dir.path());
        let mounts = SandboxMounts {
            runtime_dir: dir.path().join("rt"),
            model_file: dir.path().join("m.gguf"),
            api_key_file: dir.path().join("api.key"),
        };
        let args = vec![
            format!("{CONTAINER_RUNTIME_DIR}/llama-server"),
            "-m".into(),
            CONTAINER_MODEL_FILE.into(),
            "--api-key-file".into(),
            CONTAINER_API_KEY_FILE.into(),
        ];
        let spec = build_oci_spec(&cfg, &mounts, &args).expect("spec builds");
        assert_eq!(spec["root"]["readonly"], true);
        assert_eq!(spec["process"]["noNewPrivileges"], true);
        assert_eq!(
            spec["linux"]["resources"]["memory"]["limit"],
            DEFAULT_SANDBOX_MEMORY_LIMIT_BYTES
        );
        assert_eq!(
            spec["linux"]["resources"]["pids"]["limit"],
            DEFAULT_SANDBOX_PIDS_LIMIT
        );
        assert_eq!(spec["linux"]["seccomp"]["defaultAction"], "SCMP_ACT_ERRNO");
        let namespaces = spec["linux"]["namespaces"].as_array().expect("namespaces");
        assert!(namespaces.iter().any(|ns| ns["type"] == "network"));
        assert!(namespaces.iter().any(|ns| ns["type"] == "user"));
        let mount_list = spec["mounts"].as_array().expect("mounts");
        for dest in [
            CONTAINER_RUNTIME_DIR,
            CONTAINER_MODEL_FILE,
            CONTAINER_API_KEY_FILE,
        ] {
            let entry = mount_list
                .iter()
                .find(|m| m["destination"] == dest)
                .expect("mount present");
            let opts = entry["options"].as_array().expect("options");
            assert!(
                opts.iter().any(|o| o == "ro"),
                "mount {dest} must be read-only"
            );
        }
    }

    #[test]
    fn oci_config_refuses_external_network_or_tiny_memory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut bad_net = sample_oci_config(dir.path());
        bad_net.deny_external_network = false;
        assert!(matches!(
            bad_net.validate(),
            Err(RuntimeError::IsolationUnavailable(_))
        ));
        let mut bad_mem = sample_oci_config(dir.path());
        bad_mem.memory_limit_bytes = 1024;
        assert!(matches!(
            bad_mem.validate(),
            Err(RuntimeError::IsolationUnavailable(_))
        ));
    }

    #[test]
    fn the_development_policy_is_labeled_so_it_cannot_pass_as_isolated() {
        assert!(IsolationPolicy::UnsafeDevelopmentHost
            .require_backend()
            .is_ok());
        assert_eq!(
            IsolationPolicy::UnsafeDevelopmentHost.label(),
            "unsafe-development-host"
        );
    }
}
