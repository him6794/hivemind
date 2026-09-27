//! Shared runtime helpers for downloaded master/worker clients.
//!
//! Product model (AGENTS.md): a user-deployed master or worker should:
//! 1. start its local HTTP + bundled UI
//! 2. obtain VPN bootstrap config from the official website-api on login
//! 3. join the configured overlay automatically (embedded libtailscale on Windows)
//! 4. reach the platform nodepool over the overlay
//!
//! Users must not hand-copy pre-auth keys after install.

pub mod update;
pub mod update_loop;

use anyhow::{bail, Context, Result};
use hivemind_config::{ExternalOverlayMode, HivemindConfig};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error as _;
#[cfg(target_os = "windows")]
use std::ffi::{CStr, CString};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(target_os = "windows")]
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
#[cfg(any(test, target_os = "windows"))]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(target_os = "windows")]
use tokio::net::TcpListener;
#[cfg(target_os = "windows")]
use tokio::net::TcpStream;
#[cfg(target_os = "windows")]
use tokio::sync::watch;
use tokio::sync::Mutex as TokioMutex;
use tokio::time::sleep;
use tonic::client::Grpc;
use tonic::codec::ProstCodec;
use tonic::codegen::http::uri::PathAndQuery;
use tonic::transport::Endpoint;
use tonic::Request;

/// Official public product endpoints baked into downloaded clients.
pub const DEFAULT_WEBSITE_API_BASE: &str = "https://hivemind.justin0711.com";
pub const DEFAULT_HEADSCALE_LOGIN_SERVER: &str = "https://Headscale.justin0711.com";
/// Historical fallback VIP. Prefer peer discovery after VPN join because Headscale
/// assigns nodepool addresses dynamically and may not hand out 100.64.0.1.
pub const DEFAULT_NODEPOOL_GRPC_ENDPOINT: &str = "100.64.0.1:50051";
/// Hostname used by the platform nodepool Tailscale sidecar.
pub const DEFAULT_NODEPOOL_VPN_HOSTNAME: &str = "hivemind-nodepool";
/// Default gRPC port exposed by nodepool on the VPN overlay.
pub const DEFAULT_NODEPOOL_GRPC_PORT: u16 = 50051;
/// Default WireGuard platform public key (to be set via env or config)
pub const DEFAULT_PLATFORM_WG_PUBLIC_KEY: &str = "";
const WEBSITE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const WEBSITE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const NODEPOOL_PROBE_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Clone, PartialEq, prost::Message)]
struct TransportProbeRequest {}

#[derive(Clone, PartialEq, prost::Message)]
struct TransportProbeResponse {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientRole {
    Master,
    Worker,
}

impl ClientRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Worker => "worker",
        }
    }

    fn env_prefix(self) -> &'static str {
        match self {
            Self::Master => "MASTER",
            Self::Worker => "WORKER",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpnBootstrapPlan {
    Skip,
    Join {
        auth_key: String,
        login_server: String,
        hostname: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnBootstrapState {
    Disabled,
    AwaitingLogin,
    Joining,
    Ready,
    RetryableFailure,
    ReauthenticationRequired,
}

impl VpnBootstrapState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::AwaitingLogin => "awaiting_login",
            Self::Joining => "joining",
            Self::Ready => "ready",
            Self::RetryableFailure => "retryable_failure",
            Self::ReauthenticationRequired => "reauthentication_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnBootstrapStatus {
    pub state: VpnBootstrapState,
    pub endpoint: Option<String>,
    pub overlay_ip: Option<String>,
    pub message: Option<String>,
}

impl VpnBootstrapStatus {
    pub fn ready(endpoint: impl Into<String>, overlay_ip: Option<&str>) -> Self {
        Self {
            state: VpnBootstrapState::Ready,
            endpoint: Some(endpoint.into()),
            overlay_ip: overlay_ip.map(str::to_string),
            message: None,
        }
    }

    fn new(state: VpnBootstrapState, message: Option<String>) -> Self {
        Self {
            state,
            endpoint: None,
            overlay_ip: None,
            message,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct WebsiteLoginResponse {
    success: bool,
    #[serde(default)]
    message: String,
    token: Option<String>,
}

#[derive(Clone, Deserialize)]
struct WebsiteVpnConfigResponse {
    success: bool,
    #[serde(default)]
    login_server: String,
    #[serde(default)]
    auth_key: String,
    #[serde(default)]
    config_text: String,
}

#[derive(Debug, Clone, Serialize)]
struct WebsiteEnrollmentCredentialRequest {
    role: String,
    client_instance_id: String,
}

#[derive(Clone, Deserialize)]
struct WebsiteEnrollmentCredentialResponse {
    success: bool,
    credential: Option<String>,
}

#[derive(Clone, Serialize)]
struct WebsiteRedeemEnrollmentRequest {
    credential: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WebsiteRedeemEnrollmentResponse {
    success: bool,
    identity_id: Option<String>,
    owner: Option<String>,
    role: Option<String>,
    client_instance_id: Option<String>,
    worker_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientEnrollment {
    pub identity_id: String,
    pub owner: String,
    pub role: ClientRole,
    pub client_instance_id: String,
    pub worker_id: Option<String>,
}

/// VPN transport type - WireGuard only
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnTransport {
    Tailscale,
    Wireguard,
}

#[cfg(target_os = "windows")]
struct SocksBridge {
    addr: SocketAddr,
    shutdown: watch::Sender<bool>,
}

#[cfg(target_os = "windows")]
impl SocksBridge {
    fn addr(&self) -> SocketAddr {
        self.addr
    }

    fn close(&self) {
        self.shutdown.send_replace(true);
    }
}

#[cfg(target_os = "windows")]
impl Drop for SocksBridge {
    fn drop(&mut self) {
        self.close();
    }
}

/// VPN session state
pub struct VpnSession {
    pub role: ClientRole,
    pub transport: VpnTransport,
    pub state_dir: PathBuf,
    pub bridge_addr: Option<SocketAddr>,
    pub overlay_ip: Option<String>,
    pub auth_key: String,
    pub login_server: String,
    pub hostname: String,
    nodepool_target: String,
    worker_grpc_port: u16,
    #[cfg(target_os = "windows")]
    pub userspace_socks_addr: Option<String>,
    #[cfg(target_os = "windows")]
    pub userspace_proxy_cred: Option<String>,
    #[cfg(target_os = "windows")]
    local_api_cred: Option<String>,
    #[cfg(target_os = "windows")]
    active_bridge: StdMutex<Option<(String, Arc<SocksBridge>)>>,
    #[cfg(target_os = "windows")]
    additional_bridges: StdMutex<HashMap<String, Arc<SocksBridge>>>,
    // WireGuard specific fields
    pub wg_private_key: Option<boringtun::x25519::StaticSecret>,
    pub wg_peer_public_key: Option<boringtun::x25519::PublicKey>,
    pub wg_endpoint: Option<SocketAddr>,
    pub wg_allowed_ips: Option<String>,
    pub wg_tunnel: Option<Arc<TokioMutex<wireguard::WireguardTunnel>>>,
    #[cfg(target_os = "windows")]
    pub libtailscale: Option<Arc<LibtailscaleSession>>,
}

impl VpnSession {
    fn shutdown(&self) {
        #[cfg(target_os = "windows")]
        {
            if let Some((_, bridge)) = self
                .active_bridge
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                bridge.close();
            }
            let mut additional_bridges = self
                .additional_bridges
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for bridge in additional_bridges.values() {
                bridge.close();
            }
            additional_bridges.clear();
            if let Some(session) = &self.libtailscale {
                session.close_once();
            }
        }
    }
}

impl Drop for VpnSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(target_os = "windows")]
pub struct LibtailscaleSession {
    handle: i32,
    closed: AtomicBool,
}

#[cfg(target_os = "windows")]
impl LibtailscaleSession {
    fn close_once(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            unsafe {
                tailscale_close(self.handle);
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for LibtailscaleSession {
    fn drop(&mut self) {
        self.close_once();
    }
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
mod libtailscale_ffi {
    #[cfg(target_env = "msvc")]
    use std::ffi::OsStr;
    #[cfg(target_env = "msvc")]
    use std::os::raw::c_void;
    use std::os::raw::{c_char, c_int};
    #[cfg(target_env = "msvc")]
    use std::os::windows::ffi::OsStrExt;
    #[cfg(target_env = "msvc")]
    use std::path::PathBuf;
    #[cfg(target_env = "msvc")]
    use std::sync::OnceLock;

    type TailscaleNew = unsafe extern "C" fn() -> c_int;
    type TailscaleSetString = unsafe extern "C" fn(c_int, *const c_char) -> c_int;
    type TailscaleUp = unsafe extern "C" fn(c_int) -> c_int;
    type TailscaleClose = unsafe extern "C" fn(c_int) -> c_int;
    type TailscaleLoopback =
        unsafe extern "C" fn(c_int, *mut c_char, usize, *mut c_char, *mut c_char) -> c_int;
    type TailscaleBuffer = unsafe extern "C" fn(c_int, *mut c_char, usize) -> c_int;
    type TailscaleListenForward =
        unsafe extern "C" fn(c_int, *const c_char, *const c_char, *const c_char) -> c_int;

    #[cfg(target_env = "gnu")]
    mod static_link {
        use super::{c_char, c_int};
        extern "C" {
            pub fn tailscale_new() -> c_int;
            pub fn tailscale_set_dir(sd: c_int, dir: *const c_char) -> c_int;
            pub fn tailscale_set_hostname(sd: c_int, hostname: *const c_char) -> c_int;
            pub fn tailscale_set_authkey(sd: c_int, authkey: *const c_char) -> c_int;
            pub fn tailscale_set_control_url(sd: c_int, control_url: *const c_char) -> c_int;
            pub fn tailscale_up(sd: c_int) -> c_int;
            pub fn tailscale_close(sd: c_int) -> c_int;
            pub fn tailscale_loopback(
                sd: c_int,
                addr_out: *mut c_char,
                addrlen: usize,
                proxy_cred_out: *mut c_char,
                local_api_cred_out: *mut c_char,
            ) -> c_int;
            pub fn tailscale_getips(sd: c_int, buf: *mut c_char, buflen: usize) -> c_int;
            pub fn tailscale_listen_forward(
                sd: c_int,
                network: *const c_char,
                tailnet_addr: *const c_char,
                local_addr: *const c_char,
            ) -> c_int;
            pub fn tailscale_errmsg(sd: c_int, buf: *mut c_char, buflen: usize) -> c_int;
        }

        pub(super) fn ensure_loaded() -> Result<(), String> {
            Ok(())
        }

        pub(super) unsafe fn new() -> c_int {
            tailscale_new()
        }
        pub(super) unsafe fn set_dir(sd: c_int, value: *const c_char) -> c_int {
            tailscale_set_dir(sd, value)
        }
        pub(super) unsafe fn set_hostname(sd: c_int, value: *const c_char) -> c_int {
            tailscale_set_hostname(sd, value)
        }
        pub(super) unsafe fn set_authkey(sd: c_int, value: *const c_char) -> c_int {
            tailscale_set_authkey(sd, value)
        }
        pub(super) unsafe fn set_control_url(sd: c_int, value: *const c_char) -> c_int {
            tailscale_set_control_url(sd, value)
        }
        pub(super) unsafe fn up(sd: c_int) -> c_int {
            tailscale_up(sd)
        }
        pub(super) unsafe fn close(sd: c_int) -> c_int {
            tailscale_close(sd)
        }
        pub(super) unsafe fn loopback(
            sd: c_int,
            addr: *mut c_char,
            addr_len: usize,
            proxy: *mut c_char,
            local_api: *mut c_char,
        ) -> c_int {
            tailscale_loopback(sd, addr, addr_len, proxy, local_api)
        }
        pub(super) unsafe fn getips(sd: c_int, buf: *mut c_char, len: usize) -> c_int {
            tailscale_getips(sd, buf, len)
        }
        pub(super) unsafe fn listen_forward(
            sd: c_int,
            network: *const c_char,
            tailnet: *const c_char,
            local: *const c_char,
        ) -> c_int {
            tailscale_listen_forward(sd, network, tailnet, local)
        }
        pub(super) unsafe fn errmsg(sd: c_int, buf: *mut c_char, len: usize) -> c_int {
            tailscale_errmsg(sd, buf, len)
        }
    }

    #[cfg(target_env = "msvc")]
    mod dynamic_link {
        use super::*;

        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryW(name: *const u16) -> *mut c_void;
            fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        }

        struct Api {
            new: TailscaleNew,
            set_dir: TailscaleSetString,
            set_hostname: TailscaleSetString,
            set_authkey: TailscaleSetString,
            set_control_url: TailscaleSetString,
            up: TailscaleUp,
            close: TailscaleClose,
            loopback: TailscaleLoopback,
            getips: TailscaleBuffer,
            listen_forward: TailscaleListenForward,
            errmsg: TailscaleBuffer,
        }

        static API: OnceLock<Result<Api, String>> = OnceLock::new();

        fn dll_path() -> PathBuf {
            if let Ok(path) = std::env::var("HIVEMIND_LIBTAILSCALE_DLL") {
                return PathBuf::from(path);
            }
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(PathBuf::from))
                .unwrap_or_default()
                .join("libtailscale.dll")
        }

        unsafe fn symbol<T>(module: *mut c_void, name: &'static [u8]) -> Result<T, String> {
            let pointer = GetProcAddress(module, name.as_ptr());
            if pointer.is_null() {
                return Err(format!(
                    "libtailscale.dll is missing exported symbol {}",
                    String::from_utf8_lossy(&name[..name.len() - 1])
                ));
            }
            Ok(std::mem::transmute_copy(&pointer))
        }

        fn load() -> Result<Api, String> {
            let path = dll_path();
            let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
            let module = unsafe { LoadLibraryW(wide.as_ptr()) };
            if module.is_null() {
                return Err(format!(
                    "failed to load {}. Set HIVEMIND_LIBTAILSCALE_DLL to an explicit DLL path",
                    path.display()
                ));
            }
            unsafe {
                Ok(Api {
                    new: symbol(module, b"tailscale_new\0")?,
                    set_dir: symbol(module, b"tailscale_set_dir\0")?,
                    set_hostname: symbol(module, b"tailscale_set_hostname\0")?,
                    set_authkey: symbol(module, b"tailscale_set_authkey\0")?,
                    set_control_url: symbol(module, b"tailscale_set_control_url\0")?,
                    up: symbol(module, b"tailscale_up\0")?,
                    close: symbol(module, b"tailscale_close\0")?,
                    loopback: symbol(module, b"tailscale_loopback\0")?,
                    getips: symbol(module, b"tailscale_getips\0")?,
                    listen_forward: symbol(module, b"tailscale_listen_forward\0")?,
                    errmsg: symbol(module, b"tailscale_errmsg\0")?,
                })
            }
        }

        fn api() -> Result<&'static Api, String> {
            match API.get_or_init(load) {
                Ok(api) => Ok(api),
                Err(error) => Err(error.clone()),
            }
        }

        pub(super) fn ensure_loaded() -> Result<(), String> {
            api().map(|_| ())
        }
        pub(super) unsafe fn new() -> c_int {
            (api().expect("libtailscale must be loaded before use").new)()
        }
        pub(super) unsafe fn set_dir(sd: c_int, value: *const c_char) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .set_dir)(sd, value)
        }
        pub(super) unsafe fn set_hostname(sd: c_int, value: *const c_char) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .set_hostname)(sd, value)
        }
        pub(super) unsafe fn set_authkey(sd: c_int, value: *const c_char) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .set_authkey)(sd, value)
        }
        pub(super) unsafe fn set_control_url(sd: c_int, value: *const c_char) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .set_control_url)(sd, value)
        }
        pub(super) unsafe fn up(sd: c_int) -> c_int {
            (api().expect("libtailscale must be loaded before use").up)(sd)
        }
        pub(super) unsafe fn close(sd: c_int) -> c_int {
            (api().expect("libtailscale must be loaded before use").close)(sd)
        }
        pub(super) unsafe fn loopback(
            sd: c_int,
            addr: *mut c_char,
            addr_len: usize,
            proxy: *mut c_char,
            local_api: *mut c_char,
        ) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .loopback)(sd, addr, addr_len, proxy, local_api)
        }
        pub(super) unsafe fn getips(sd: c_int, buf: *mut c_char, len: usize) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .getips)(sd, buf, len)
        }
        pub(super) unsafe fn listen_forward(
            sd: c_int,
            network: *const c_char,
            tailnet: *const c_char,
            local: *const c_char,
        ) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .listen_forward)(sd, network, tailnet, local)
        }
        pub(super) unsafe fn errmsg(sd: c_int, buf: *mut c_char, len: usize) -> c_int {
            (api()
                .expect("libtailscale must be loaded before use")
                .errmsg)(sd, buf, len)
        }
    }

    pub(super) fn ensure_loaded() -> Result<(), String> {
        #[cfg(target_env = "gnu")]
        {
            static_link::ensure_loaded()
        }
        #[cfg(target_env = "msvc")]
        {
            dynamic_link::ensure_loaded()
        }
    }

    pub(super) unsafe fn tailscale_new() -> c_int {
        #[cfg(target_env = "gnu")]
        {
            static_link::new()
        }
        #[cfg(target_env = "msvc")]
        {
            dynamic_link::new()
        }
    }
    macro_rules! delegate {
        ($name:ident, $gnu:ident, $msvc:ident, ($($arg:ident: $ty:ty),*)) => {
            pub(super) unsafe fn $name($($arg: $ty),*) -> c_int {
                #[cfg(target_env = "gnu")]
                { static_link::$gnu($($arg),*) }
                #[cfg(target_env = "msvc")]
                { dynamic_link::$msvc($($arg),*) }
            }
        };
    }
    delegate!(tailscale_set_dir, set_dir, set_dir, (sd: c_int, value: *const c_char));
    delegate!(tailscale_set_hostname, set_hostname, set_hostname, (sd: c_int, value: *const c_char));
    delegate!(tailscale_set_authkey, set_authkey, set_authkey, (sd: c_int, value: *const c_char));
    delegate!(tailscale_set_control_url, set_control_url, set_control_url, (sd: c_int, value: *const c_char));
    delegate!(tailscale_up, up, up, (sd: c_int));
    delegate!(tailscale_close, close, close, (sd: c_int));
    delegate!(tailscale_getips, getips, getips, (sd: c_int, buf: *mut c_char, len: usize));
    delegate!(tailscale_errmsg, errmsg, errmsg, (sd: c_int, buf: *mut c_char, len: usize));

    pub(super) unsafe fn tailscale_loopback(
        sd: c_int,
        addr: *mut c_char,
        addr_len: usize,
        proxy: *mut c_char,
        local_api: *mut c_char,
    ) -> c_int {
        #[cfg(target_env = "gnu")]
        {
            static_link::loopback(sd, addr, addr_len, proxy, local_api)
        }
        #[cfg(target_env = "msvc")]
        {
            dynamic_link::loopback(sd, addr, addr_len, proxy, local_api)
        }
    }

    pub(super) unsafe fn tailscale_listen_forward(
        sd: c_int,
        network: *const c_char,
        tailnet: *const c_char,
        local: *const c_char,
    ) -> c_int {
        #[cfg(target_env = "gnu")]
        {
            static_link::listen_forward(sd, network, tailnet, local)
        }
        #[cfg(target_env = "msvc")]
        {
            dynamic_link::listen_forward(sd, network, tailnet, local)
        }
    }
}

#[cfg(target_os = "windows")]
use libtailscale_ffi::{
    ensure_loaded as ensure_libtailscale_loaded, tailscale_close, tailscale_errmsg,
    tailscale_getips, tailscale_listen_forward, tailscale_loopback, tailscale_new,
    tailscale_set_authkey, tailscale_set_control_url, tailscale_set_dir, tailscale_set_hostname,
    tailscale_up,
};

impl VpnSession {
    /// Get the verified active bridge, never a candidate that has not passed gRPC probing.
    pub fn bridge_endpoint(&self) -> Option<String> {
        #[cfg(target_os = "windows")]
        if let Some((_, bridge)) = self
            .active_bridge
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            return Some(bridge.addr().to_string());
        }
        self.bridge_addr.map(|addr| addr.to_string())
    }

    fn active_nodepool_target(&self) -> Option<String> {
        #[cfg(target_os = "windows")]
        if let Some((target, _)) = self
            .active_bridge
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            return Some(target.clone());
        }
        self.bridge_addr.map(|_| self.nodepool_target.clone())
    }

    #[cfg(target_os = "windows")]
    fn activate_bridge(&self, target: String, bridge: Arc<SocksBridge>) {
        let previous = self
            .active_bridge
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .replace((target, bridge));
        if let Some((_, previous)) = previous {
            previous.close();
        }
    }
}

const VPN_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
const VPN_KEEPALIVE_FAILURE_THRESHOLD: u32 = 3;
const VPN_KEEPALIVE_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Process-local recovery information. It deliberately has no `Debug` implementation:
/// an enrollment key may be retained only in memory to recover an authenticated tunnel.
#[derive(Clone)]
struct VpnReconnectPlan {
    role: ClientRole,
    auth_key: Option<String>,
    login_server: String,
    hostname: String,
    configured_endpoint: String,
    advertised_endpoint: Option<String>,
    operator_endpoint: bool,
    worker_grpc_addr: Option<String>,
    startup_timeout: Duration,
    require_external_overlay: bool,
}

impl VpnReconnectPlan {
    fn new(
        role: ClientRole,
        auth_key: Option<&str>,
        login_server: &str,
        hostname: &str,
        configured_endpoint: &str,
        worker_grpc_addr: Option<&str>,
        startup_timeout: Duration,
        require_external_overlay: bool,
    ) -> Self {
        Self {
            role,
            auth_key: auth_key.map(str::to_string),
            login_server: login_server.trim_end_matches('/').to_string(),
            hostname: bounded_hostname(hostname),
            configured_endpoint: normalize_nodepool_endpoint(configured_endpoint),
            advertised_endpoint: None,
            operator_endpoint: false,
            worker_grpc_addr: worker_grpc_addr.map(str::to_string),
            startup_timeout: startup_timeout.max(Duration::from_secs(1)),
            require_external_overlay,
        }
    }

    fn with_operator_endpoint(mut self, config: &HivemindConfig) -> Self {
        self.operator_endpoint = operator_nodepool_endpoint_configured(config);
        self
    }
}

struct VpnRuntime {
    session: Option<Arc<VpnSession>>,
    generation: u64,
    reconnect_plan: Option<VpnReconnectPlan>,
    keepalive_started: bool,
}

impl Default for VpnRuntime {
    fn default() -> Self {
        Self {
            session: None,
            generation: 0,
            reconnect_plan: None,
            keepalive_started: false,
        }
    }
}

/// All mutable lifecycle state for a role is kept together so that a new tunnel,
/// bridge, recovery plan, and keepalive cannot be published independently.
static VPN_RUNTIMES: OnceLock<StdMutex<HashMap<ClientRole, VpnRuntime>>> = OnceLock::new();
static VPN_STATUSES: OnceLock<StdMutex<HashMap<ClientRole, VpnBootstrapStatus>>> = OnceLock::new();
static VPN_BOOTSTRAP_LOCKS: OnceLock<StdMutex<HashMap<ClientRole, Arc<TokioMutex<()>>>>> =
    OnceLock::new();

fn runtimes_map() -> &'static StdMutex<HashMap<ClientRole, VpnRuntime>> {
    VPN_RUNTIMES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn statuses_map() -> &'static StdMutex<HashMap<ClientRole, VpnBootstrapStatus>> {
    VPN_STATUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn bootstrap_locks_map() -> &'static StdMutex<HashMap<ClientRole, Arc<TokioMutex<()>>>> {
    VPN_BOOTSTRAP_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn bootstrap_lock(role: ClientRole) -> Arc<TokioMutex<()>> {
    let mut locks = bootstrap_locks_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks
        .entry(role)
        .or_insert_with(|| Arc::new(TokioMutex::new(())))
        .clone()
}

fn set_vpn_status(role: ClientRole, status: VpnBootstrapStatus) {
    statuses_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(role, status);
}

/// Return the non-secret current bootstrap state for a client role.
pub fn current_vpn_status(role: ClientRole) -> VpnBootstrapStatus {
    statuses_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&role)
        .cloned()
        .unwrap_or_else(|| VpnBootstrapStatus::new(VpnBootstrapState::AwaitingLogin, None))
}

fn install_vpn_session(session: Arc<VpnSession>, plan: VpnReconnectPlan) -> Arc<VpnSession> {
    let previous = {
        let mut runtimes = runtimes_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let runtime = runtimes.entry(session.role).or_default();
        runtime.generation = runtime.generation.wrapping_add(1);
        runtime.reconnect_plan = Some(plan);
        runtime.session.replace(session.clone())
    };
    if let Some(previous) = previous {
        previous.shutdown();
    }
    session
}

fn vpn_runtime_snapshot(
    role: ClientRole,
) -> (Option<Arc<VpnSession>>, u64, Option<VpnReconnectPlan>) {
    let runtimes = runtimes_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(runtime) = runtimes.get(&role) else {
        return (None, 0, None);
    };
    (
        runtime.session.clone(),
        runtime.generation,
        runtime.reconnect_plan.clone(),
    )
}

fn claim_vpn_keepalive(role: ClientRole) -> bool {
    let mut runtimes = runtimes_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let runtime = runtimes.entry(role).or_default();
    if runtime.keepalive_started {
        false
    } else {
        runtime.keepalive_started = true;
        true
    }
}

fn release_vpn_keepalive(role: ClientRole) {
    if let Some(runtime) = runtimes_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get_mut(&role)
    {
        runtime.keepalive_started = false;
    }
}

fn retire_vpn_session_if_generation(role: ClientRole, generation: u64) -> bool {
    let previous = {
        let mut runtimes = runtimes_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let runtime = runtimes.entry(role).or_default();
        if runtime.generation != generation {
            return false;
        }
        runtime.generation = runtime.generation.wrapping_add(1);
        runtime.session.take()
    };
    if let Some(previous) = previous {
        previous.shutdown();
    }
    true
}

/// Get the current VPN session for a role.
pub async fn current_vpn_session(role: ClientRole) -> Option<Arc<VpnSession>> {
    vpn_runtime_snapshot(role).0
}

/// Return whether this client must use the authenticated external overlay.
///
/// Local is deliberately the configuration default because the compose topology
/// uses a directly published Nodepool endpoint. Strict mode is an explicit
/// validation boundary; it never turns a direct endpoint into overlay evidence.
pub fn external_overlay_required(config: &HivemindConfig, _role: ClientRole) -> bool {
    config.vpn.external_overlay_mode == ExternalOverlayMode::Strict
}

/// Validate the configuration needed before a strict external enrollment.
///
/// This check is intentionally independent of endpoint reachability. A local
/// Docker address, a disabled Website API, or an insecure Website API URL must
/// not be accepted as a substitute for the authenticated external path.
pub fn validate_external_overlay_configuration(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<()> {
    if !external_overlay_required(config, role) {
        return Ok(());
    }
    if !cfg!(target_os = "windows") {
        bail!("strict external overlay requires a native Windows client");
    }
    if env_truthy("HIVEMIND_DISABLE_WEBSITE_VPN")
        || env_truthy(&format!("{}_DISABLE_WEBSITE_VPN", role.env_prefix()))
    {
        bail!("strict external overlay rejects disabled Website API enrollment");
    }
    let website_base = website_api_base(config, role).ok_or_else(|| {
        anyhow::anyhow!("strict external overlay requires Website API enrollment")
    })?;
    if !website_base.starts_with("https://") {
        bail!("strict external overlay requires an HTTPS Website API endpoint");
    }
    let login_server = first_nonempty(&[
        env_trim(&format!("{}_VPN_LOGIN_SERVER", role.env_prefix())),
        env_trim("HEADSCALE_LOGIN_SERVER"),
        Some(config.vpn.headscale_login_server.trim().to_string())
            .filter(|value| !value.is_empty()),
        Some(config.vpn.headscale_url.trim().to_string()).filter(|value| !value.is_empty()),
    ])
    .ok_or_else(|| anyhow::anyhow!("strict external overlay requires a Headscale login server"))?;
    if !login_server.starts_with("https://") {
        bail!("strict external overlay requires an HTTPS Headscale login server");
    }
    Ok(())
}

/// Resolve a Nodepool endpoint only through an active authenticated overlay.
///
/// Unlike `resolve_reachable_nodepool_endpoint`, this function never probes or
/// returns the configured direct endpoint. The active libtailscale session and
/// its localhost bridge are both required before the transport probe is used.
pub async fn external_overlay_endpoint(
    role: ClientRole,
    configured_endpoint: &str,
) -> Result<String> {
    let session = current_vpn_session(role)
        .await
        .ok_or_else(|| anyhow::anyhow!("authenticated overlay session is not ready"))?;
    if session.transport != VpnTransport::Tailscale {
        bail!("strict external overlay requires the embedded libtailscale transport");
    }
    if session
        .overlay_ip
        .as_deref()
        .is_none_or(|ip| ip.trim().is_empty())
    {
        bail!("authenticated overlay session has no assigned overlay address");
    }
    let bridge = session
        .bridge_endpoint()
        .ok_or_else(|| anyhow::anyhow!("authenticated overlay session has no local bridge"))?;
    if !nodepool_endpoint_reachable(&bridge).await {
        bail!(
            "authenticated overlay Nodepool transport is unavailable (configured endpoint: {})",
            configured_endpoint
        );
    }
    Ok(bridge)
}

/// Validate and resolve the strict external overlay before an operation.
pub async fn ensure_external_overlay_ready(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<String> {
    validate_external_overlay_configuration(config, role)?;
    external_overlay_endpoint(role, &resolve_nodepool_grpc_endpoint(config)).await
}

/// Decide whether a status response may claim VPN bootstrap success.
///
/// In strict mode `Disabled` is never success, and a status without an overlay
/// address cannot be evidence of an authenticated external session.
pub fn vpn_bootstrap_status_success(
    config: &HivemindConfig,
    role: ClientRole,
    status: &VpnBootstrapStatus,
) -> bool {
    if external_overlay_required(config, role) {
        status.state == VpnBootstrapState::Ready
            && status
                .overlay_ip
                .as_deref()
                .is_some_and(|ip| !ip.trim().is_empty())
    } else {
        matches!(
            status.state,
            VpnBootstrapState::Ready | VpnBootstrapState::Disabled
        )
    }
}

/// Retire the current VPN bridge and libtailscale handle for a role.
///
/// The recovery plan remains in memory so the singleton keepalive can restore
/// the session after a genuine transport loss. No credential is written or logged.
pub async fn clear_vpn_session(role: ClientRole) {
    let previous = {
        let mut runtimes = runtimes_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let runtime = runtimes.entry(role).or_default();
        runtime.generation = runtime.generation.wrapping_add(1);
        runtime.session.take()
    };
    if let Some(previous) = previous {
        previous.shutdown();
    }
}

fn disable_vpn_runtime_for_direct_endpoint(role: ClientRole) {
    let previous = {
        let mut runtimes = runtimes_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let runtime = runtimes.entry(role).or_default();
        runtime.generation = runtime.generation.wrapping_add(1);
        runtime.reconnect_plan = None;
        runtime.session.take()
    };
    if let Some(previous) = previous {
        previous.shutdown();
    }
}

/// Resolve whether a client should join the platform VPN from explicit settings.
///
/// Opt-in is the auth key. A bare platform `HEADSCALE_LOGIN_SERVER` must not
/// force every colocated process onto the VPN.
pub fn plan_vpn_bootstrap(
    auth_key: Option<&str>,
    login_server: Option<&str>,
    hostname: Option<&str>,
    config_login_server: Option<&str>,
    role: ClientRole,
) -> Result<VpnBootstrapPlan> {
    let auth_key = auth_key.map(str::trim).filter(|v| !v.is_empty());
    let mut login_server = login_server.map(str::trim).filter(|v| !v.is_empty());
    if auth_key.is_some() && login_server.is_none() {
        login_server = config_login_server.map(str::trim).filter(|v| !v.is_empty());
    }
    let hostname = hostname
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", role.as_str(), short_host_id()));

    match auth_key {
        None => Ok(VpnBootstrapPlan::Skip),
        Some(auth_key) => match login_server {
            None => bail!(
                "{}_VPN_LOGIN_SERVER or HEADSCALE_LOGIN_SERVER is required when {}_VPN_AUTHKEY is set",
                role.env_prefix(),
                role.env_prefix()
            ),
            Some(login_server) => Ok(VpnBootstrapPlan::Join {
                auth_key: auth_key.to_string(),
                login_server: login_server.trim_end_matches('/').to_string(),
                hostname,
            }),
        },
    }
}

fn worker_grpc_addr_for_role(config: &HivemindConfig, role: ClientRole) -> Option<&str> {
    match role {
        ClientRole::Worker => Some(config.server.worker_grpc_addr.as_str()),
        ClientRole::Master => None,
    }
}

/// Best-effort startup bootstrap when an operator already provisioned an auth key.
///
/// This is intentionally a no-op for typical downloaded clients. Those obtain a
/// preauth key automatically during login via website-api.
pub async fn ensure_env_vpn(config: &HivemindConfig, role: ClientRole) -> Result<Option<String>> {
    let configured_endpoint = resolve_nodepool_grpc_endpoint(config);
    ensure_env_vpn_for_endpoint(config, role, &configured_endpoint).await
}

/// Bootstrap a role-specific Headscale session for a selected Nodepool endpoint.
///
/// Returning the effective endpoint is important on Windows: embedded libtailscale
/// exposes a local SOCKS bridge because ordinary gRPC sockets cannot route through
/// the userspace TUN directly. `None` means no explicit auth key was configured.
pub async fn ensure_env_vpn_for_endpoint(
    config: &HivemindConfig,
    role: ClientRole,
    configured_endpoint: &str,
) -> Result<Option<String>> {
    let worker_grpc_addr = worker_grpc_addr_for_role(config, role);
    ensure_env_vpn_for_endpoint_with_worker_addr(
        config,
        role,
        configured_endpoint,
        worker_grpc_addr,
    )
    .await
}

async fn ensure_env_vpn_for_endpoint_with_worker_addr(
    config: &HivemindConfig,
    role: ClientRole,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
) -> Result<Option<String>> {
    let lock = bootstrap_lock(role);
    let _guard = lock.lock().await;
    ensure_env_vpn_for_endpoint_with_worker_addr_locked(
        config,
        role,
        configured_endpoint,
        worker_grpc_addr,
    )
    .await
}

async fn ensure_env_vpn_for_endpoint_with_worker_addr_locked(
    config: &HivemindConfig,
    role: ClientRole,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
) -> Result<Option<String>> {
    let prefix = role.env_prefix();
    let require_external_overlay = external_overlay_required(config, role);
    let auth_key = first_nonempty(&[
        env_trim(&format!("{prefix}_VPN_AUTHKEY")),
        env_trim(&format!("{prefix}_VPN_AUTH_KEY")),
        env_trim("TS_AUTHKEY"),
    ]);
    let login_server = first_nonempty(&[
        env_trim(&format!("{prefix}_VPN_LOGIN_SERVER")),
        env_trim("HEADSCALE_LOGIN_SERVER"),
        Some(config.vpn.headscale_login_server.trim().to_string()).filter(|v| !v.is_empty()),
        Some(DEFAULT_HEADSCALE_LOGIN_SERVER.to_string()),
    ]);
    let hostname = first_nonempty(&[
        env_trim(&format!("{prefix}_VPN_HOSTNAME")),
        env_trim("HOSTNAME"),
        env_trim("COMPUTERNAME"),
        Some(format!("{}-{}", role.as_str(), short_host_id())),
    ]);

    if require_external_overlay {
        validate_external_overlay_configuration(config, role)?;
    }

    match plan_vpn_bootstrap(
        auth_key.as_deref(),
        login_server.as_deref(),
        hostname.as_deref(),
        Some(config.vpn.headscale_url.as_str()),
        role,
    )? {
        VpnBootstrapPlan::Skip => {
            if require_external_overlay {
                set_vpn_status(
                    role,
                    VpnBootstrapStatus::new(
                        VpnBootstrapState::AwaitingLogin,
                        Some("sign in to enroll this client on the external overlay".into()),
                    ),
                );
            }
            tracing::info!(
                "{} VPN env bootstrap skipped (no {}_VPN_AUTHKEY); login may auto-issue via website-api",
                role.as_str(),
                prefix
            );
            Ok(None)
        }
        VpnBootstrapPlan::Join {
            auth_key,
            login_server,
            hostname,
        } => {
            if require_external_overlay && !login_server.starts_with("https://") {
                bail!("strict external overlay requires an HTTPS Headscale login server");
            }
            let reconnect_plan = VpnReconnectPlan::new(
                role,
                Some(&auth_key),
                &login_server,
                &hostname,
                configured_endpoint,
                worker_grpc_addr,
                Duration::from_secs(config.vpn.startup_timeout_secs),
                require_external_overlay,
            )
            .with_operator_endpoint(config);
            match reusable_vpn_endpoint(&reconnect_plan).await {
                Ok(Some(endpoint)) => {
                    set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
                    return Ok(Some(endpoint));
                }
                Ok(None) => {}
                Err(err) => {
                    set_vpn_status(
                        role,
                        VpnBootstrapStatus::new(
                            VpnBootstrapState::RetryableFailure,
                            Some(err.to_string()),
                        ),
                    );
                    return Err(err);
                }
            }

            // A candidate libtailscale instance uses the role's persistent state
            // directory. Retire an incompatible instance before opening it.
            if current_vpn_session(role).await.is_some() {
                clear_vpn_session(role).await;
            }
            let endpoint = if persisted_vpn_identity_matches(&reconnect_plan) {
                match join_and_confirm_nodepool(&reconnect_plan, None).await {
                    Ok(endpoint) => endpoint,
                    Err(err) => {
                        clear_vpn_session(role).await;
                        tracing::warn!(
                            "{} persisted VPN state could not rehydrate for explicit auth-key startup; resetting local state: {}",
                            role.as_str(),
                            err
                        );
                        reset_libtailscale_state_for_new_auth_key(role)?;
                        join_and_confirm_nodepool(&reconnect_plan, Some(&auth_key)).await?
                    }
                }
            } else {
                reset_libtailscale_state_for_new_auth_key(role)?;
                join_and_confirm_nodepool(&reconnect_plan, Some(&auth_key)).await?
            };
            set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
            Ok(Some(endpoint))
        }
    }
}

/// Automatic VPN join for a logged-in user using the official website-api.
///
/// This compatibility wrapper accepts credentials for the initial login path,
/// then delegates the actual enrollment to the token-only helper. Passwords
/// never enter the VPN config request.
pub async fn ensure_user_vpn(
    config: &HivemindConfig,
    role: ClientRole,
    username: &str,
    password: &str,
    existing_token: Option<&str>,
) -> Result<Option<String>> {
    let lock = bootstrap_lock(role);
    let _guard = lock.lock().await;
    ensure_user_vpn_inner(config, role, username, password, existing_token).await
}

/// Rehydrate or enroll a role using an already-issued Nodepool JWT.
///
/// The JWT is sent only to the protected Website API. The returned Headscale
/// auth key is consumed by this process and is never returned to the caller.
pub async fn ensure_user_vpn_for_token(
    config: &HivemindConfig,
    role: ClientRole,
    token: &str,
) -> Result<Option<String>> {
    let lock = bootstrap_lock(role);
    let _guard = lock.lock().await;
    ensure_user_vpn_for_token_inner(config, role, token).await
}

/// Return the stable, non-secret client instance identifier used for enrollment
/// and outbound session binding.
pub fn client_instance_id(role: ClientRole) -> Result<String> {
    persisted_device_id(role)
}

/// Obtain and immediately redeem a short-lived server enrollment credential.
///
/// The credential is held only in this call's stack and is never written to
/// the local VPN state. Nodepool assigns or recovers the client identity.
pub async fn ensure_client_enrollment(
    config: &HivemindConfig,
    role: ClientRole,
    token: &str,
) -> Result<ClientEnrollment> {
    let token = token.trim();
    if token.is_empty() {
        bail!("a non-empty bearer token is required for enrollment");
    }
    let website_base = website_api_base(config, role)
        .ok_or_else(|| anyhow::anyhow!("website-api enrollment is disabled"))?;
    let client_instance_id = persisted_device_id(role)?;
    let credential =
        website_issue_enrollment_credential(&website_base, token, role, &client_instance_id)
            .await?;
    let enrollment = website_redeem_enrollment_credential(&website_base, &credential).await?;
    if enrollment.role != role {
        bail!("enrollment credential role does not match client role");
    }
    if enrollment.client_instance_id != client_instance_id {
        bail!("enrollment credential is bound to a different client instance");
    }
    if role == ClientRole::Worker && enrollment.worker_id.is_none() {
        bail!("worker enrollment did not return a server-assigned worker identity");
    }
    Ok(enrollment)
}

async fn ensure_user_vpn_inner(
    config: &HivemindConfig,
    role: ClientRole,
    username: &str,
    password: &str,
    existing_token: Option<&str>,
) -> Result<Option<String>> {
    let require_external_overlay = external_overlay_required(config, role);
    if require_external_overlay {
        validate_external_overlay_configuration(config, role)?;
    }
    if env_auth_key_present(role) {
        let endpoint = ensure_env_vpn_for_endpoint_with_worker_addr_locked(
            config,
            role,
            &resolve_nodepool_grpc_endpoint(config),
            worker_grpc_addr_for_role(config, role),
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("explicit VPN auth key disappeared during bootstrap"))?;
        return Ok(Some(endpoint));
    }

    let Some(website_base) = website_api_base(config, role) else {
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(VpnBootstrapState::Disabled, None),
        );
        tracing::debug!(
            "{} website-api base disabled; skipping automatic website VPN bootstrap",
            role.as_str()
        );
        return Ok(None);
    };

    let token = match existing_token.map(str::trim).filter(|v| !v.is_empty()) {
        Some(token) => token.to_string(),
        None => website_login(&website_base, username, password).await?,
    };
    ensure_user_vpn_for_token_inner(config, role, &token).await
}

async fn ensure_user_vpn_for_token_inner(
    config: &HivemindConfig,
    role: ClientRole,
    token: &str,
) -> Result<Option<String>> {
    let require_external_overlay = external_overlay_required(config, role);
    if require_external_overlay {
        validate_external_overlay_configuration(config, role)?;
    }
    let token = token.trim();
    if token.is_empty() {
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(
                VpnBootstrapState::ReauthenticationRequired,
                Some("a non-empty bearer token is required".into()),
            ),
        );
        bail!("a non-empty bearer token is required for VPN bootstrap");
    }

    if env_auth_key_present(role) {
        let endpoint = ensure_env_vpn_for_endpoint_with_worker_addr_locked(
            config,
            role,
            &resolve_nodepool_grpc_endpoint(config),
            worker_grpc_addr_for_role(config, role),
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("explicit VPN auth key disappeared during bootstrap"))?;
        return Ok(Some(endpoint));
    }

    let Some(website_base) = website_api_base(config, role) else {
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(VpnBootstrapState::Disabled, None),
        );
        return Ok(None);
    };

    let configured_endpoint = resolve_nodepool_grpc_endpoint(config);
    if !require_external_overlay {
        let direct = if operator_nodepool_endpoint_configured(config)
            && nodepool_endpoint_reachable(&configured_endpoint).await
        {
            Some(configured_endpoint.clone())
        } else if !operator_nodepool_endpoint_configured(config) {
            first_reachable_nodepool_endpoint(role, &configured_endpoint).await
        } else {
            None
        };
        if let Some(endpoint) = direct {
            if operator_nodepool_endpoint_configured(config) {
                // A previous overlay must not let the Master/Worker refresh path
                // replace this explicitly selected, reachable local endpoint.
                disable_vpn_runtime_for_direct_endpoint(role);
            }
            set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
            return Ok(Some(endpoint));
        }
    }

    let device_name = client_device_name(role)?;
    let login_server = first_nonempty(&[
        env_trim(&format!("{}_VPN_LOGIN_SERVER", role.env_prefix())),
        env_trim("HEADSCALE_LOGIN_SERVER"),
        Some(config.vpn.headscale_login_server.clone()).filter(|v| !v.trim().is_empty()),
        Some(config.vpn.headscale_url.clone()).filter(|v| !v.trim().is_empty()),
        Some(DEFAULT_HEADSCALE_LOGIN_SERVER.to_string()),
    ])
    .ok_or_else(|| anyhow::anyhow!("no Headscale login server is configured"))?;
    // Website enrollment always uses the bounded role/device label. Rehydrate
    // with that same Headscale identity rather than a process/host alias.
    let hostname = bounded_hostname(&device_name);
    let rehydrate_plan = VpnReconnectPlan::new(
        role,
        None,
        &login_server,
        &hostname,
        &configured_endpoint,
        worker_grpc_addr_for_role(config, role),
        Duration::from_secs(config.vpn.startup_timeout_secs),
        require_external_overlay,
    )
    .with_operator_endpoint(config);
    match reusable_vpn_endpoint(&rehydrate_plan).await {
        Ok(Some(endpoint)) => {
            set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
            return Ok(Some(endpoint));
        }
        Ok(None) => {}
        Err(err) => {
            set_vpn_status(
                role,
                VpnBootstrapStatus::new(VpnBootstrapState::RetryableFailure, Some(err.to_string())),
            );
            return Err(err);
        }
    }

    // A candidate libtailscale instance uses the role's persistent state
    // directory. Retire an incompatible instance before opening it.
    if current_vpn_session(role).await.is_some() {
        clear_vpn_session(role).await;
    }
    set_vpn_status(
        role,
        VpnBootstrapStatus::new(VpnBootstrapState::Joining, None),
    );

    // A successful libtailscale state can reconnect without issuing another
    // one-time key. If that state is stale or revoked, fall through to the
    // authenticated Website API issuance path below.
    if persisted_vpn_identity_matches(&rehydrate_plan) {
        match join_and_confirm_nodepool(&rehydrate_plan, None).await {
            Ok(endpoint) => {
                set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
                return Ok(Some(endpoint));
            }
            Err(err) => {
                clear_vpn_session(role).await;
                tracing::warn!(
                    "{} persisted VPN state could not rehydrate; requesting a fresh enrollment key: {}",
                    role.as_str(),
                    err
                );
            }
        }
    }

    let vpn = match website_issue_vpn_config(&website_base, token, &device_name).await {
        Ok(vpn) => vpn,
        Err(err) => {
            let message = err.to_string();
            let state = if message.contains("401")
                || message.to_ascii_lowercase().contains("unauthorized")
                || message.to_ascii_lowercase().contains("token")
            {
                VpnBootstrapState::ReauthenticationRequired
            } else {
                VpnBootstrapState::RetryableFailure
            };
            set_vpn_status(role, VpnBootstrapStatus::new(state, Some(message.clone())));
            return Err(err);
        }
    };

    if vpn.auth_key.trim().is_empty() {
        let err = anyhow::anyhow!("website-api VPN config did not include an auth_key");
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(VpnBootstrapState::RetryableFailure, Some(err.to_string())),
        );
        return Err(err);
    }

    let login_server = first_nonempty(&[
        Some(vpn.login_server.clone()).filter(|v| !v.trim().is_empty()),
        Some(login_server),
    ])
    .ok_or_else(|| anyhow::anyhow!("website-api VPN config did not include login_server"))?;
    // The Website API client ID is user-scoped and can exceed Headscale's
    // 63-character DNS-label limit once sanitized. The persisted role/device
    // label is already stable and bounded, so use it for the actual node name.
    let join_hostname = bounded_hostname(&device_name);

    let mut fresh_plan = VpnReconnectPlan::new(
        role,
        Some(vpn.auth_key.trim()),
        login_server.trim_end_matches('/'),
        &join_hostname,
        &configured_endpoint,
        worker_grpc_addr_for_role(config, role),
        Duration::from_secs(config.vpn.startup_timeout_secs),
        require_external_overlay,
    )
    .with_operator_endpoint(config);
    if !fresh_plan.operator_endpoint {
        fresh_plan.advertised_endpoint = parse_advertised_nodepool_endpoint(&vpn.config_text);
    }

    // A newly issued key must not be ignored by tsnet because a previous
    // failed/revoked session left a NeedsLogin state file behind.
    clear_vpn_session(role).await;
    reset_libtailscale_state_for_new_auth_key(role)?;

    match join_and_confirm_nodepool(&fresh_plan, fresh_plan.auth_key.as_deref()).await {
        Ok(endpoint) => {
            set_ready_vpn_status(role, &endpoint, require_external_overlay).await?;
            Ok(Some(endpoint))
        }
        Err(err) => {
            let message = err.to_string();
            set_vpn_status(
                role,
                VpnBootstrapStatus::new(VpnBootstrapState::RetryableFailure, Some(message)),
            );
            Err(err)
        }
    }
}

fn session_matches_reconnect_plan(session: &VpnSession, plan: &VpnReconnectPlan) -> bool {
    session.role == plan.role
        && session.login_server.trim_end_matches('/') == plan.login_server
        && session.hostname == plan.hostname
        && session.nodepool_target == plan.configured_endpoint
        && session.worker_grpc_port
            == endpoint_port_for_worker(plan.role, plan.worker_grpc_addr.as_deref())
}

async fn session_ready_endpoint(session: &VpnSession, plan: &VpnReconnectPlan) -> Option<String> {
    if plan.require_external_overlay
        && (session.transport != VpnTransport::Tailscale
            || session
                .overlay_ip
                .as_deref()
                .is_none_or(|ip| ip.trim().is_empty()))
    {
        return None;
    }
    if let Some(bridge) = session.bridge_endpoint() {
        if nodepool_endpoint_reachable(&bridge).await {
            return Some(bridge);
        }
    }
    if !plan.require_external_overlay
        && nodepool_endpoint_reachable(&plan.configured_endpoint).await
    {
        return Some(plan.configured_endpoint.clone());
    }
    None
}

/// Reuse a matching session only after its current bridge passes the gRPC
/// transport probe. A transient probe failure is left for the debounced
/// keepalive to repair rather than replacing libtailscale during login.
async fn reusable_vpn_endpoint(plan: &VpnReconnectPlan) -> Result<Option<String>> {
    let Some(session) = current_vpn_session(plan.role).await else {
        return Ok(None);
    };
    if !session_matches_reconnect_plan(session.as_ref(), plan) {
        return Ok(None);
    }
    if let Some(endpoint) = session_ready_endpoint(session.as_ref(), plan).await {
        return Ok(Some(endpoint));
    }
    bail!(
        "VPN bootstrap: {} session matches the requested overlay but its Nodepool bridge is unavailable; background recovery will retry",
        plan.role.as_str()
    )
}

async fn set_ready_vpn_status(
    role: ClientRole,
    endpoint: &str,
    require_external_overlay: bool,
) -> Result<()> {
    let session = current_vpn_session(role).await;
    let strict_session = session.as_ref().is_some_and(|session| {
        session.transport == VpnTransport::Tailscale
            && session
                .overlay_ip
                .as_deref()
                .is_some_and(|ip| !ip.trim().is_empty())
    });
    if require_external_overlay && !strict_session {
        let error = anyhow::anyhow!(
            "strict external overlay cannot report Nodepool readiness without an authenticated Tailscale session"
        );
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(VpnBootstrapState::RetryableFailure, Some(error.to_string())),
        );
        return Err(error);
    }

    let overlay_ip = session.and_then(|session| session.overlay_ip.clone());
    set_vpn_status(
        role,
        VpnBootstrapStatus {
            state: VpnBootstrapState::Ready,
            endpoint: Some(endpoint.to_string()),
            overlay_ip,
            message: None,
        },
    );
    Ok(())
}

pub fn website_api_base(config: &HivemindConfig, role: ClientRole) -> Option<String> {
    if env_truthy("HIVEMIND_DISABLE_WEBSITE_VPN")
        || env_truthy(&format!("{}_DISABLE_WEBSITE_VPN", role.env_prefix()))
    {
        return None;
    }

    first_nonempty(&[
        env_trim(&format!("{}_WEBSITE_API_BASE", role.env_prefix())),
        env_trim("WEBSITE_API_BASE"),
        env_trim("HIVEMIND_WEBSITE_API_BASE"),
        // Only use configured website HTTP addr when it looks like a client endpoint,
        // not a bind address for a local website-api process.
        Some(config.server.website_http_addr.clone())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .filter(|v| !v.starts_with("0.0.0.0:") && !v.starts_with("[::]:")),
        Some(DEFAULT_WEBSITE_API_BASE.to_string()),
    ])
    .map(|base| normalize_http_base(&base))
}

fn operator_nodepool_endpoint_configured(config: &HivemindConfig) -> bool {
    config
        .server
        .nodepool_grpc_endpoint
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        || {
            let addr = config.server.nodepool_grpc_addr.trim();
            !addr.is_empty() && !addr.starts_with("0.0.0.0:") && !addr.starts_with("[::]:")
        }
}

/// Only accept a well-formed overlay address from the authenticated Website
/// config. Ignore unrelated lines (including private keys) and never include
/// the source text in a log or error.
fn parse_advertised_nodepool_endpoint(config_text: &str) -> Option<String> {
    let mut advertised = None;
    for line in config_text.lines() {
        let Some(value) = line.trim().strip_prefix("# nodepool_grpc_endpoint=") else {
            continue;
        };
        if advertised.is_some() {
            return None;
        }
        advertised = Some(value.trim());
    }
    validate_overlay_endpoint(advertised?)
}

fn validate_overlay_endpoint(value: &str) -> Option<String> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '\\' | '@' | '?' | '#'))
    {
        return None;
    }
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (host, remainder) = rest.split_once(']')?;
        (host, remainder.strip_prefix(':')?)
    } else {
        value.rsplit_once(':')?
    };
    let port = port.parse::<u16>().ok()?;
    if port == 0 || host.is_empty() {
        return None;
    }
    let valid_host = match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            let [first, second, ..] = ip.octets();
            first == 100 && (64..128).contains(&second)
        }
        Ok(IpAddr::V6(ip)) => ip.is_unique_local(),
        Err(_) => {
            let hostname = host.trim_end_matches('.');
            hostname.eq_ignore_ascii_case(DEFAULT_NODEPOOL_VPN_HOSTNAME)
                || hostname
                    .to_ascii_lowercase()
                    .starts_with(&format!("{DEFAULT_NODEPOOL_VPN_HOSTNAME}."))
        }
    };
    valid_host.then(|| value.to_string())
}

/// Resolve the nodepool gRPC endpoint for downloaded clients.
///
/// Preference order:
/// 1. explicit `NODEPOOL_GRPC_ENDPOINT`
/// 2. non-bind `NODEPOOL_GRPC_ADDR`
/// 3. historical platform VIP fallback (runtime discovery prefers the live
///    WireGuard peer address for `hivemind-nodepool`)
pub fn resolve_nodepool_grpc_endpoint(config: &HivemindConfig) -> String {
    if let Some(endpoint) = config
        .server
        .nodepool_grpc_endpoint
        .as_ref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
    {
        return endpoint.to_string();
    }

    let addr = config.server.nodepool_grpc_addr.trim();
    if !addr.is_empty() && !addr.starts_with("0.0.0.0:") && !addr.starts_with("[::]:") {
        return addr.to_string();
    }

    DEFAULT_NODEPOOL_GRPC_ENDPOINT.to_string()
}

/// Normalize a configured nodepool host/port for consumers that add their own
/// transport scheme, such as the Windows userspace SOCKS bridge.
pub fn normalize_nodepool_endpoint(endpoint: &str) -> String {
    endpoint
        .trim()
        .strip_prefix("http://")
        .or_else(|| endpoint.trim().strip_prefix("https://"))
        .unwrap_or(endpoint.trim())
        .trim_end_matches('/')
        .to_string()
}

///
/// Explicit operator overrides still win when they answer TCP. Otherwise the
/// client looks up the platform nodepool WireGuard peer and uses its overlay IP.
pub async fn resolve_reachable_nodepool_endpoint(
    role: ClientRole,
    configured_endpoint: &str,
) -> Result<String> {
    if let Some(endpoint) =
        first_reachable_nodepool_endpoint_with_mode(role, configured_endpoint, false).await
    {
        return Ok(endpoint);
    }

    let session = current_vpn_session(role).await;
    let candidates =
        nodepool_endpoint_candidates(role, configured_endpoint, session.as_deref(), false).await;
    bail!(
        "nodepool endpoint is still unreachable after VPN bootstrap (tried: {}). Check that WireGuard is connected and that the platform nodepool VPN sidecar ({}) is online",
        if candidates.is_empty() {
            configured_endpoint.to_string()
        } else {
            candidates.join(", ")
        },
        DEFAULT_NODEPOOL_VPN_HOSTNAME
    )
}

async fn first_reachable_nodepool_endpoint(
    role: ClientRole,
    configured_endpoint: &str,
) -> Option<String> {
    first_reachable_nodepool_endpoint_with_mode(role, configured_endpoint, false).await
}

async fn first_reachable_nodepool_endpoint_with_mode(
    role: ClientRole,
    configured_endpoint: &str,
    require_external_overlay: bool,
) -> Option<String> {
    let session = current_vpn_session(role).await;
    first_reachable_nodepool_endpoint_with_session(
        role,
        configured_endpoint,
        session.as_deref(),
        require_external_overlay,
    )
    .await
}

async fn first_reachable_nodepool_endpoint_with_session(
    role: ClientRole,
    configured_endpoint: &str,
    session: Option<&VpnSession>,
    require_external_overlay: bool,
) -> Option<String> {
    let candidates =
        nodepool_endpoint_candidates(role, configured_endpoint, session, require_external_overlay)
            .await;
    // Probe candidates concurrently. Sequential probes made every login wait
    // for dead overlay/DNS candidates before trying the live one.
    let mut probes = tokio::task::JoinSet::new();
    for candidate in candidates {
        probes.spawn(async move {
            if nodepool_endpoint_reachable(&candidate).await {
                Some(candidate)
            } else {
                None
            }
        });
    }
    while let Some(result) = probes.join_next().await {
        if let Ok(Some(candidate)) = result {
            probes.abort_all();
            if candidate != configured_endpoint {
                tracing::info!(
                    "{} discovered reachable nodepool endpoint {} (configured was {})",
                    role.as_str(),
                    candidate,
                    configured_endpoint
                );
            }
            return Some(candidate);
        }
    }
    None
}

async fn nodepool_endpoint_candidates(
    _role: ClientRole,
    configured_endpoint: &str,
    session: Option<&VpnSession>,
    require_external_overlay: bool,
) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut push_unique = |value: String| {
        let value = value.trim().trim_end_matches('/').to_string();
        if value.is_empty() {
            return;
        }
        if !candidates.iter().any(|existing| existing == &value) {
            candidates.push(value);
        }
    };

    // Local userspace TCP bridge first: ordinary gRPC sockets cannot use the
    // userspace TUN, so we expose nodepool on a localhost forwarder. Once a
    // bridge exists, never bypass it with the raw endpoint after a keyed join.
    if let Some(session) = session {
        let strict_session = session.transport == VpnTransport::Tailscale
            && session
                .overlay_ip
                .as_deref()
                .is_some_and(|ip| !ip.trim().is_empty());
        if !require_external_overlay || strict_session {
            if let Some(bridge) = session.bridge_endpoint() {
                push_unique(bridge);
                return candidates;
            }
        }
    }

    if require_external_overlay {
        // Strict external mode has no direct-endpoint compatibility path. A
        // missing authenticated bridge is a not-ready state, not evidence.
        return candidates;
    }

    // The configured endpoint is authoritative when the active transport does
    // not require a userspace bridge (for example, a future kernel WireGuard
    // integration or direct/no-key local development).
    push_unique(configured_endpoint.to_string());
    candidates
}

/// Convert a listen/bind address into a browser URL on localhost when needed.
pub fn local_ui_url(listen_addr: &str) -> String {
    let addr = listen_addr.trim();
    let host_port = if let Some(rest) = addr.strip_prefix("http://") {
        rest
    } else if let Some(rest) = addr.strip_prefix("https://") {
        rest
    } else {
        addr
    };

    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, port),
        None => (host_port, "80"),
    };

    let browser_host = if host.is_empty()
        || host == "0.0.0.0"
        || host == "[::]"
        || host == "::"
        || host.eq_ignore_ascii_case("localhost")
    {
        "127.0.0.1"
    } else {
        host.trim_start_matches('[').trim_end_matches(']')
    };

    format!("http://{browser_host}:{port}/")
}

/// Best-effort browser open for local master/worker UIs.
pub fn open_ui_in_browser(url: &str) -> Result<()> {
    if env_truthy("HIVEMIND_DISABLE_OPEN_UI") {
        tracing::info!("UI browser open disabled via HIVEMIND_DISABLE_OPEN_UI");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to open UI in browser: {url}"))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("failed to open UI in browser: {url}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux desktop environments. Ignore failure in headless CI/server boxes.
        if std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_err()
        {
            tracing::debug!("xdg-open unavailable; UI is still served at {url}");
        }
    }

    tracing::info!("Opened local UI at {url}");
    Ok(())
}

pub async fn open_ui_when_ready(listen_addr: &str) {
    let url = local_ui_url(listen_addr);
    // Give the listener a brief moment to bind before launching a browser.
    sleep(Duration::from_millis(350)).await;
    if let Err(err) = open_ui_in_browser(&url) {
        tracing::warn!("Failed to open local UI browser window: {err}");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientUiRole {
    Master,
    Worker,
}

impl ClientUiRole {
    fn label(self) -> &'static str {
        match self {
            Self::Master => "Master",
            Self::Worker => "Worker",
        }
    }

    #[cfg(any(test, target_os = "windows"))]
    fn windows_executables(self) -> (&'static str, &'static str) {
        match self {
            Self::Master => ("hivemind-master.exe", "hivemind-master-ui.exe"),
            Self::Worker => ("hivemind-worker.exe", "hivemind-worker-ui.exe"),
        }
    }
}

fn local_browser_url(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V6(ip) if ip.ip().is_unspecified() => {
            format!("http://[::1]:{}/", ip.port())
        }
        SocketAddr::V6(_) => format!("http://{addr}/"),
        SocketAddr::V4(_) => local_ui_url(&addr.to_string()),
    }
}

#[cfg(any(test, target_os = "windows"))]
fn local_webview_url(addr: SocketAddr, ui_available: bool, ui_disabled: bool) -> Option<String> {
    if ui_disabled || !ui_available || addr.port() == 0 {
        return None;
    }
    match addr.ip() {
        std::net::IpAddr::V4(ip) if ip == Ipv4Addr::LOCALHOST || ip.is_unspecified() => {
            Some(format!("http://127.0.0.1:{}/", addr.port()))
        }
        _ => None,
    }
}

#[cfg(any(test, target_os = "windows"))]
const LOCAL_UI_READY_MARKER: &[u8] = b"HIVEMIND_LOCAL_UI_READY\n";
#[cfg(any(test, target_os = "windows"))]
const LOCAL_UI_READY_TIMEOUT: Duration = Duration::from_secs(8);
#[cfg(any(test, target_os = "windows"))]
const LOCAL_UI_ENV_ALLOWLIST: &[&str] = &[
    "SystemRoot",
    "WINDIR",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "TEMP",
    "TMP",
    "PATH",
    "ProgramFiles",
    "ProgramFiles(x86)",
];

#[cfg(any(test, target_os = "windows"))]
fn filtered_local_ui_environment<I>(
    environment: I,
) -> impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>
where
    I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
{
    environment.into_iter().filter(|(name, _)| {
        LOCAL_UI_ENV_ALLOWLIST.iter().any(|allowed| {
            name.to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(allowed))
        })
    })
}

#[cfg(any(test, target_os = "windows"))]
async fn await_local_ui_readiness<R>(reader: &mut R, timeout: Duration) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut marker = [0; LOCAL_UI_READY_MARKER.len()];
    match tokio::time::timeout(timeout, reader.read_exact(&mut marker)).await {
        Ok(Ok(_)) if marker == LOCAL_UI_READY_MARKER => Ok(()),
        outcome => bail!("local UI helper did not become ready: {outcome:?}"),
    }
}

#[cfg(any(test, target_os = "windows"))]
fn retain_local_ui_lifetime_pipe<F, P>(helper_wait: F, lifetime_pipe: P, role: &'static str)
where
    F: std::future::Future<Output = std::io::Result<()>> + Send + 'static,
    P: Send + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = helper_wait.await {
            tracing::warn!("{role} WebView process wait failed: {error}");
        }
        drop(lifetime_pipe);
    });
}

#[cfg(any(test, target_os = "windows"))]
async fn open_ui_with_browser_fallback<W, WF, B>(
    role: ClientUiRole,
    webview_url: Option<&str>,
    browser_url: &str,
    launch_webview: W,
    open_browser: B,
) where
    W: FnOnce(String) -> WF,
    WF: std::future::Future<Output = Result<()>>,
    B: FnOnce(&str) -> Result<()>,
{
    if let Some(url) = webview_url {
        match launch_webview(url.to_owned()).await {
            Ok(()) => return,
            Err(error) => tracing::warn!(
                "{} WebView unavailable, opening browser: {error}",
                role.label()
            ),
        }
    }

    if let Err(error) = open_browser(browser_url) {
        tracing::warn!(
            "Failed to open local {} UI browser window: {error}",
            role.label()
        );
    }
}

/// Open the packaged Master UI in a Windows WebView, falling back to the browser.
pub async fn open_master_ui_when_ready(addr: SocketAddr, ui_available: bool) {
    open_local_ui_when_ready(addr, ui_available, ClientUiRole::Master).await;
}

/// Open the packaged Worker UI in a Windows WebView, falling back to the browser.
/// The window runs in a separate process so closing it cannot stop active work.
pub async fn open_worker_ui_when_ready(addr: SocketAddr, ui_available: bool) {
    open_local_ui_when_ready(addr, ui_available, ClientUiRole::Worker).await;
}

async fn open_local_ui_when_ready(addr: SocketAddr, ui_available: bool, role: ClientUiRole) {
    sleep(Duration::from_millis(350)).await;
    let disabled = env_truthy("HIVEMIND_DISABLE_OPEN_UI");
    if disabled {
        tracing::info!(
            "{} UI open disabled via HIVEMIND_DISABLE_OPEN_UI",
            role.label()
        );
        return;
    }

    let browser_url = local_browser_url(addr);

    #[cfg(target_os = "windows")]
    open_ui_with_browser_fallback(
        role,
        local_webview_url(addr, ui_available, disabled).as_deref(),
        &browser_url,
        |url| async move { launch_client_webview(role, &url).await },
        open_ui_in_browser,
    )
    .await;

    #[cfg(not(target_os = "windows"))]
    {
        let _ = ui_available;
        if let Err(error) = open_ui_in_browser(&browser_url) {
            tracing::warn!(
                "Failed to open local {} UI browser window: {error}",
                role.label()
            );
        }
    }
}

#[cfg(target_os = "windows")]
async fn launch_client_webview(role: ClientUiRole, url: &str) -> Result<()> {
    let (client_name, helper_name) = role.windows_executables();
    let client_exe = std::env::current_exe().context("cannot locate client executable")?;
    let is_expected_client = client_exe
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(client_name));
    if !is_expected_client {
        bail!(
            "embedded window is only available for the dedicated {} executable",
            role.label()
        );
    }
    let helper = client_exe.with_file_name(helper_name);
    if !helper.is_file() {
        bail!("packaged {} WebView helper is missing", role.label());
    }

    let mut command = tokio::process::Command::new(&helper);
    command
        .arg(url)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    // GUI helpers never inherit credentials, VPN keys, or runtime configuration.
    command.envs(filtered_local_ui_environment(std::env::vars_os()));

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start {} WebView helper: {}",
            role.label(),
            helper.display()
        )
    })?;
    let mut stdout = child
        .stdout
        .take()
        .context("WebView readiness pipe missing")?;
    if let Err(error) = await_local_ui_readiness(&mut stdout, LOCAL_UI_READY_TIMEOUT).await {
        let _ = child.start_kill();
        return Err(error)
            .with_context(|| format!("{} WebView did not become ready", role.label()));
    }

    tracing::info!("Opened {} WebView at {url}", role.label());
    // Keep stdin open while waiting so the separate helper remains alive until
    // this client exits or the user closes the helper window.
    let stdin = child
        .stdin
        .take()
        .context("WebView lifetime pipe missing")?;
    retain_local_ui_lifetime_pipe(
        async move { child.wait().await.map(|_| ()) },
        stdin,
        role.label(),
    );
    Ok(())
}

fn env_auth_key_present(role: ClientRole) -> bool {
    let prefix = role.env_prefix();
    first_nonempty(&[
        env_trim(&format!("{prefix}_VPN_AUTHKEY")),
        env_trim(&format!("{prefix}_VPN_AUTH_KEY")),
        env_trim("TS_AUTHKEY"),
    ])
    .is_some()
}

async fn website_login(base: &str, username: &str, password: &str) -> Result<String> {
    let client = website_http_client()?;
    let response = client
        .post(format!("{base}/api/login"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
        }))
        .send()
        .await
        .context("website-api login request failed")?;
    let status = response.status();
    let raw = response.text().await?;
    let body: WebsiteLoginResponse = serde_json::from_str(&raw).with_context(|| {
        format!(
            "website-api login returned HTTP {} with invalid JSON: {}",
            status,
            truncate_response_body(&raw)
        )
    })?;
    if !status.is_success() || !body.success {
        bail!("website-api login failed: {}", body.message);
    }
    body.token
        .ok_or_else(|| anyhow::anyhow!("login succeeded but no token returned"))
}

async fn website_issue_enrollment_credential(
    base: &str,
    token: &str,
    role: ClientRole,
    client_instance_id: &str,
) -> Result<String> {
    let client = website_http_client()?;
    let response = client
        .post(format!("{base}/api/enrollment/credential"))
        .bearer_auth(token)
        .json(&WebsiteEnrollmentCredentialRequest {
            role: role.as_str().into(),
            client_instance_id: client_instance_id.into(),
        })
        .send()
        .await
        .context("website-api enrollment credential request failed")?;
    let status = response.status();
    let raw = response.text().await?;
    let body: WebsiteEnrollmentCredentialResponse =
        serde_json::from_str(&raw).with_context(|| {
            format!("website-api enrollment returned HTTP {status} with invalid JSON")
        })?;
    if !status.is_success() || !body.success {
        bail!("website-api enrollment credential request was rejected");
    }
    body.credential
        .filter(|credential| !credential.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("enrollment succeeded but no credential returned"))
}

async fn website_redeem_enrollment_credential(
    base: &str,
    credential: &str,
) -> Result<ClientEnrollment> {
    let client = website_http_client()?;
    let response = client
        .post(format!("{base}/api/enrollment/redeem"))
        .json(&WebsiteRedeemEnrollmentRequest {
            credential: credential.to_string(),
        })
        .send()
        .await
        .context("website-api enrollment redemption request failed")?;
    let status = response.status();
    let raw = response.text().await?;
    let body: WebsiteRedeemEnrollmentResponse = serde_json::from_str(&raw).with_context(|| {
        format!("website-api enrollment redemption returned HTTP {status} with invalid JSON")
    })?;
    if !status.is_success() || !body.success {
        bail!("website-api enrollment redemption was rejected");
    }
    let role = match body.role.as_deref() {
        Some("master") => ClientRole::Master,
        Some("worker") => ClientRole::Worker,
        _ => bail!("enrollment redemption returned an invalid role"),
    };
    Ok(ClientEnrollment {
        identity_id: body
            .identity_id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("enrollment redemption returned no identity"))?,
        owner: body
            .owner
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("enrollment redemption returned no owner"))?,
        role,
        client_instance_id: body
            .client_instance_id
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("enrollment redemption returned no client identity"))?,
        worker_id: body.worker_id.filter(|value| !value.trim().is_empty()),
    })
}

async fn website_issue_vpn_config(
    base: &str,
    token: &str,
    client_name: &str,
) -> Result<WebsiteVpnConfigResponse> {
    let client = website_http_client()?;
    let response = client
        .post(format!("{base}/api/vpn/config"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "client_name": client_name,
        }))
        .send()
        .await
        .context("website-api VPN config request failed")?;
    let status = response.status();
    let raw = response.text().await?;
    let body: WebsiteVpnConfigResponse = serde_json::from_str(&raw).with_context(|| {
        format!("website-api VPN config returned HTTP {status} with invalid JSON")
    })?;
    if !status.is_success() || !body.success {
        bail!("website-api VPN config failed with HTTP {status}");
    }
    Ok(body)
}

fn truncate_response_body(body: &str) -> String {
    let compact = body.trim().replace(['\r', '\n'], " ");
    if compact.chars().count() > 240 {
        format!("{}…", compact.chars().take(240).collect::<String>())
    } else {
        compact
    }
}

fn website_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(WEBSITE_CONNECT_TIMEOUT)
        .timeout(WEBSITE_REQUEST_TIMEOUT)
        .build()
        .context("failed to create website-api HTTP client")
}

async fn bring_up_vpn_bounded(
    plan: &VpnReconnectPlan,
    auth_key: Option<&str>,
) -> Result<Arc<VpnSession>> {
    tokio::time::timeout(plan.startup_timeout, bring_up_vpn(plan, auth_key))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "VPN bootstrap: {} startup exceeded the configured timeout of {:?}",
                plan.role.as_str(),
                plan.startup_timeout
            )
        })?
}

/// Build a candidate userspace tunnel, prove its bridge reaches Nodepool, then
/// publish it atomically. Failed candidates never replace the active session.
async fn join_and_confirm_nodepool(
    plan: &VpnReconnectPlan,
    auth_key: Option<&str>,
) -> Result<String> {
    let startup_deadline = Instant::now() + plan.startup_timeout;
    let session = bring_up_vpn_bounded(plan, auth_key).await?;
    let remaining = startup_deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!(
            "VPN bootstrap: {} startup exceeded the configured timeout of {:?} before Nodepool readiness",
            plan.role.as_str(),
            plan.startup_timeout
        );
    }
    let endpoint = match wait_for_nodepool_after_join(session.as_ref(), plan, remaining).await {
        Ok(endpoint) => endpoint,
        Err(error) => {
            session.shutdown();
            return Err(error);
        }
    };
    let target = session
        .active_nodepool_target()
        .ok_or_else(|| anyhow::anyhow!("VPN/Nodepool transport has no verified remote target"))?;

    // Do not publish the candidate until its authenticated bridge has completed
    // the same HTTP/2 transport handshake used by the Worker and Master.
    install_vpn_session(session, plan.clone());
    if let Err(err) =
        mark_persisted_vpn_state(plan.role, &plan.login_server, &plan.hostname, &target)
    {
        tracing::warn!(
            "{} VPN joined but its local state marker could not be persisted: {}",
            plan.role.as_str(),
            err
        );
    }
    spawn_vpn_keepalive_once(plan.role);
    Ok(endpoint)
}

fn spawn_vpn_keepalive_once(role: ClientRole) {
    if !claim_vpn_keepalive(role) {
        return;
    }
    tokio::spawn(async move {
        vpn_keepalive_loop(role).await;
        release_vpn_keepalive(role);
    });
}

fn should_reconnect_vpn(failures: u32) -> bool {
    failures >= VPN_KEEPALIVE_FAILURE_THRESHOLD
}

async fn vpn_keepalive_loop(role: ClientRole) {
    let mut failures = 0u32;
    let mut reconnect_backoff = Duration::from_secs(1);
    let mut next_reconnect = Instant::now();

    loop {
        sleep(VPN_KEEPALIVE_INTERVAL).await;
        let (session, generation, plan) = vpn_runtime_snapshot(role);
        let Some(plan) = plan else {
            continue;
        };
        let endpoint = match session.as_deref() {
            Some(session) if session_matches_reconnect_plan(session, &plan) => {
                session_ready_endpoint(session, &plan).await
            }
            _ => None,
        };
        if let Some(endpoint) = endpoint {
            if failures > 0 {
                tracing::info!(
                    "{} VPN keepalive restored through {}",
                    role.as_str(),
                    endpoint
                );
                if let Err(err) =
                    set_ready_vpn_status(role, &endpoint, plan.require_external_overlay).await
                {
                    tracing::warn!("{} VPN readiness status was rejected: {err}", role.as_str());
                }
            }
            failures = 0;
            reconnect_backoff = Duration::from_secs(1);
            next_reconnect = Instant::now();
            continue;
        }

        failures = failures.saturating_add(1);
        if !should_reconnect_vpn(failures) {
            set_vpn_status(
                role,
                VpnBootstrapStatus::new(
                    VpnBootstrapState::RetryableFailure,
                    Some(format!(
                        "Nodepool readiness probe failed ({failures}/{VPN_KEEPALIVE_FAILURE_THRESHOLD}); retaining VPN session"
                    )),
                ),
            );
            tracing::warn!(
                "{} VPN keepalive missed Nodepool (streak={failures}); retaining session until recovery threshold",
                role.as_str()
            );
            continue;
        }
        if Instant::now() < next_reconnect {
            continue;
        }

        // A browser/login request and the keepalive must never build overlapping
        // libtailscale instances. Recheck inside the same per-role bootstrap
        // lock so a stale observation cannot tear down a newly restored tunnel.
        let lock = bootstrap_lock(role);
        let _guard = lock.lock().await;
        let (current_session, current_generation, current_plan) = vpn_runtime_snapshot(role);
        let Some(current_plan) = current_plan else {
            continue;
        };
        if current_generation != generation
            || current_plan.configured_endpoint != plan.configured_endpoint
        {
            continue;
        }
        if let Some(current_session) = current_session {
            if session_matches_reconnect_plan(current_session.as_ref(), &current_plan)
                && session_ready_endpoint(current_session.as_ref(), &current_plan)
                    .await
                    .is_some()
            {
                failures = 0;
                reconnect_backoff = Duration::from_secs(1);
                next_reconnect = Instant::now();
                continue;
            }
        }
        if !retire_vpn_session_if_generation(role, generation) {
            continue;
        }

        set_vpn_status(
            role,
            VpnBootstrapStatus::new(
                VpnBootstrapState::RetryableFailure,
                Some("Nodepool readiness lost; reconnecting VPN".into()),
            ),
        );
        tracing::warn!(
            "{} VPN keepalive reached recovery threshold; reconnecting userspace VPN",
            role.as_str()
        );
        match join_and_confirm_nodepool(&current_plan, current_plan.auth_key.as_deref()).await {
            Ok(endpoint) => {
                failures = 0;
                reconnect_backoff = Duration::from_secs(1);
                next_reconnect = Instant::now();
                if let Err(err) =
                    set_ready_vpn_status(role, &endpoint, current_plan.require_external_overlay)
                        .await
                {
                    tracing::warn!(
                        "{} VPN reconnect readiness status was rejected: {err}",
                        role.as_str()
                    );
                }
            }
            Err(err) => {
                set_vpn_status(
                    role,
                    VpnBootstrapStatus::new(
                        VpnBootstrapState::RetryableFailure,
                        Some(err.to_string()),
                    ),
                );
                tracing::warn!("{} VPN reconnect failed: {err}", role.as_str());
                next_reconnect = Instant::now() + reconnect_backoff;
                reconnect_backoff =
                    (reconnect_backoff + reconnect_backoff).min(VPN_KEEPALIVE_MAX_BACKOFF);
            }
        }
    }
}

async fn wait_for_nodepool_after_join(
    session: &VpnSession,
    plan: &VpnReconnectPlan,
    timeout: Duration,
) -> Result<String> {
    if plan.require_external_overlay {
        if session.transport != VpnTransport::Tailscale {
            bail!("strict external overlay requires the embedded libtailscale transport");
        }
        if session
            .overlay_ip
            .as_deref()
            .is_none_or(|ip| ip.trim().is_empty())
        {
            bail!("authenticated overlay session has no assigned overlay address");
        }
    }

    // The advertised (or saved) target gets one short probe. Re-query
    // Headscale peers while its netmap settles, bounded independently of the
    // longer VPN join timeout so one stale address cannot stall every login.
    let deadline = Instant::now() + timeout.min(Duration::from_secs(12));
    let advertised_target = if plan.operator_endpoint {
        Some(plan.configured_endpoint.clone())
    } else {
        plan.advertised_endpoint
            .clone()
            .or_else(|| persisted_nodepool_target(plan))
    };
    if let Some(target) = advertised_target {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            if let Some(endpoint) = probe_nodepool_target(session, &target, remaining).await? {
                return Ok(endpoint);
            }
        }
    }
    if plan.operator_endpoint {
        bail!(
            "VPN bootstrap: nodepool endpoint {} explicitly configured by the operator failed its gRPC transport probe; no automatic reroute was attempted",
            plan.configured_endpoint
        );
    }

    let mut last_peer_error = None;
    while Instant::now() < deadline {
        match nodepool_peer_targets(session).await {
            Ok(peers) => {
                for target in peers {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    if let Some(endpoint) =
                        probe_nodepool_target(session, &target, remaining).await?
                    {
                        return Ok(endpoint);
                    }
                }
            }
            Err(error) => last_peer_error = Some(error),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            sleep(remaining.min(Duration::from_millis(500))).await;
        }
    }
    bail!(
        "VPN bootstrap: nodepool endpoint unavailable; no advertised endpoint or online {} peer completed the gRPC transport handshake{}",
        DEFAULT_NODEPOOL_VPN_HOSTNAME,
        last_peer_error
            .map(|error| format!(" (peer discovery: {error})"))
            .unwrap_or_default()
    )
}

async fn nodepool_peer_targets(session: &VpnSession) -> Result<Vec<String>> {
    #[cfg(target_os = "windows")]
    {
        let loopback = session
            .userspace_socks_addr
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("VPN LocalAPI loopback address is unavailable"))?;
        let credential = session
            .local_api_cred
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("VPN LocalAPI credential is unavailable"))?;
        let status = local_api_status(loopback, credential).await?;
        return Ok(extract_nodepool_peer_ips(
            &status,
            &[DEFAULT_NODEPOOL_VPN_HOSTNAME.to_string()],
        )
        .into_iter()
        .map(|ip| format!("{ip}:{DEFAULT_NODEPOOL_GRPC_PORT}"))
        .collect());
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = session;
        Ok(Vec::new())
    }
}

#[cfg(target_os = "windows")]
async fn local_api_status(loopback: &str, credential: &str) -> Result<serde_json::Value> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .context("VPN LocalAPI HTTP client is unavailable")?;
    let response = client
        .get(format!("http://{loopback}/localapi/v0/status?peers=true"))
        .header("Sec-Tailscale", "localapi")
        .basic_auth("", Some(credential))
        .send()
        .await
        .context("VPN LocalAPI status request failed")?;
    if !response.status().is_success() {
        bail!(
            "VPN LocalAPI status request returned HTTP {}",
            response.status()
        );
    }
    response
        .json()
        .await
        .context("VPN LocalAPI returned invalid status JSON")
}

async fn probe_nodepool_target(
    session: &VpnSession,
    target: &str,
    remaining: Duration,
) -> Result<Option<String>> {
    let probe_timeout = remaining.min(Duration::from_secs(3));
    #[cfg(target_os = "windows")]
    if let (Some(socks), Some(credential)) = (
        session.userspace_socks_addr.as_deref(),
        session.userspace_proxy_cred.as_deref(),
    ) {
        let bridge = start_socks_bridge(socks, credential, target).await?;
        let endpoint = bridge.addr().to_string();
        let reachable = tokio::time::timeout(probe_timeout, nodepool_endpoint_reachable(&endpoint))
            .await
            .unwrap_or(false);
        if reachable {
            session.activate_bridge(target.to_string(), bridge);
            tracing::info!(
                "{} VPN/Nodepool gRPC transport verified at {}",
                session.role.as_str(),
                target
            );
            return Ok(Some(endpoint));
        }
        bridge.close();
        return Ok(None);
    }
    if session.transport == VpnTransport::Tailscale {
        bail!("VPN/Nodepool userspace SOCKS transport is unavailable");
    }
    Ok(
        tokio::time::timeout(probe_timeout, nodepool_endpoint_reachable(target))
            .await
            .unwrap_or(false)
            .then(|| target.to_string()),
    )
}

/// Start the bundled Tailscale userspace VPN and expose its overlay through a
/// localhost SOCKS bridge. This is required on Windows, where userspace mode
/// does not install a kernel route for ordinary gRPC sockets.
async fn bring_up_vpn(plan: &VpnReconnectPlan, auth_key: Option<&str>) -> Result<Arc<VpnSession>> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (plan, auth_key);
        bail!("embedded libtailscale is currently only packaged for Windows");
    }

    #[cfg(target_os = "windows")]
    {
        bring_up_vpn_windows(plan, auth_key).await
    }
}

#[cfg(target_os = "windows")]
async fn bring_up_vpn_windows(
    plan: &VpnReconnectPlan,
    auth_key: Option<&str>,
) -> Result<Arc<VpnSession>> {
    ensure_libtailscale_loaded().map_err(|error| anyhow::anyhow!(error))?;
    let role = plan.role;
    let hostname = sanitize_hostname(&plan.hostname);
    let state_dir = vpn_state_dir(role);
    std::fs::create_dir_all(&state_dir).with_context(|| {
        format!(
            "failed to create {} VPN state dir {}",
            role.as_str(),
            state_dir.display()
        )
    })?;

    let (vpn_handle, loopback_addr, proxy_cred, local_api_cred, overlay_ip) =
        start_libtailscale(&state_dir, &hostname, auth_key, &plan.login_server).await?;
    let worker_grpc_port = endpoint_port_for_worker(role, plan.worker_grpc_addr.as_deref());
    let network = CString::new("tcp")?;
    let tailnet_addr = CString::new(format!(":{worker_grpc_port}"))?;
    let local_addr = CString::new(format!("127.0.0.1:{worker_grpc_port}"))?;
    if role == ClientRole::Worker
        && unsafe {
            tailscale_listen_forward(
                vpn_handle.handle,
                network.as_ptr(),
                tailnet_addr.as_ptr(),
                local_addr.as_ptr(),
            )
        } != 0
    {
        bail!("embedded libtailscale could not expose worker execution port");
    }
    tracing::info!(
        "{} VPN joined via embedded libtailscale; nodepool target {}",
        role.as_str(),
        plan.configured_endpoint
    );
    let session = VpnSession {
        role,
        transport: VpnTransport::Tailscale,
        state_dir,
        bridge_addr: None,
        overlay_ip,
        auth_key: auth_key.unwrap_or_default().to_string(),
        login_server: plan.login_server.clone(),
        hostname: hostname.to_string(),
        nodepool_target: plan.configured_endpoint.clone(),
        worker_grpc_port,
        userspace_socks_addr: Some(loopback_addr),
        userspace_proxy_cred: Some(proxy_cred),
        local_api_cred: Some(local_api_cred),
        active_bridge: StdMutex::new(None),
        additional_bridges: StdMutex::new(HashMap::new()),
        wg_private_key: None,
        wg_peer_public_key: None,
        wg_endpoint: None,
        wg_allowed_ips: None,
        wg_tunnel: None,
        libtailscale: Some(vpn_handle),
    };
    Ok(Arc::new(session))
}

/// Return a localhost endpoint forwarding through the embedded userspace VPN.
/// On platforms without an embedded userspace session, preserve the direct endpoint.
pub async fn userspace_tcp_bridge(role: ClientRole, target: &str) -> Result<String> {
    let Some(session) = current_vpn_session(role).await else {
        return Ok(target.to_string());
    };
    #[cfg(target_os = "windows")]
    if let (Some(socks), Some(cred)) = (
        session.userspace_socks_addr.as_deref(),
        session.userspace_proxy_cred.as_deref(),
    ) {
        let bridge_target = normalize_nodepool_endpoint(target);
        if session.active_nodepool_target().as_deref() == Some(bridge_target.as_str()) {
            return session
                .bridge_endpoint()
                .ok_or_else(|| anyhow::anyhow!("active userspace session has no Nodepool bridge"));
        }
        if let Some(bridge) = session
            .additional_bridges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&bridge_target)
            .cloned()
        {
            return Ok(bridge.addr().to_string());
        }
        let bridge = start_socks_bridge(socks, cred, &bridge_target).await?;
        let bridge = session
            .additional_bridges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(bridge_target)
            .or_insert(bridge)
            .clone();
        return Ok(bridge.addr().to_string());
    }
    #[cfg(not(target_os = "windows"))]
    let _ = session;
    Ok(target.to_string())
}

#[allow(dead_code)]
fn endpoint_port_for_worker(role: ClientRole, configured_worker_addr: Option<&str>) -> u16 {
    if role != ClientRole::Worker {
        return 50053;
    }
    let configured_worker_addr = configured_worker_addr
        .map(str::to_string)
        .or_else(|| std::env::var("WORKER_GRPC_ADDR").ok());
    configured_worker_addr
        .as_deref()
        .and_then(|addr| {
            normalize_nodepool_endpoint(addr)
                .rsplit_once(':')
                .map(|(_, port)| port.to_string())
        })
        .and_then(|port| port.parse().ok())
        .unwrap_or(50053)
}

#[cfg(target_os = "windows")]
async fn start_libtailscale(
    state_dir: &Path,
    hostname: &str,
    auth_key: Option<&str>,
    login_server: &str,
) -> Result<(
    Arc<LibtailscaleSession>,
    String,
    String,
    String,
    Option<String>,
)> {
    let state_dir = state_dir.to_path_buf();
    let hostname = CString::new(hostname)?;
    let auth_key = auth_key.map(CString::new).transpose()?;
    let login_server = CString::new(login_server)?;
    tokio::task::spawn_blocking(move || {
        let handle = unsafe { tailscale_new() };
        if handle < 0 {
            bail!("libtailscale failed to allocate a session");
        }
        let fail = |message: &str| -> anyhow::Error {
            let mut buf = vec![0i8; 2048];
            let detail = if unsafe { tailscale_errmsg(handle, buf.as_mut_ptr(), buf.len()) } == 0 {
                unsafe { CStr::from_ptr(buf.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
            anyhow::anyhow!(
                "{message}{}",
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            )
        };
        let dir = CString::new(state_dir.to_string_lossy().as_bytes())?;
        for (ok, name) in [
            (
                unsafe { tailscale_set_dir(handle, dir.as_ptr()) },
                "set state dir",
            ),
            (
                unsafe { tailscale_set_hostname(handle, hostname.as_ptr()) },
                "set hostname",
            ),
        ] {
            if ok != 0 {
                let err = fail(name);
                unsafe { tailscale_close(handle) };
                return Err(err);
            }
        }
        if let Some(auth_key) = auth_key.as_ref() {
            if unsafe { tailscale_set_authkey(handle, auth_key.as_ptr()) } != 0 {
                let err = fail("set auth key");
                unsafe { tailscale_close(handle) };
                return Err(err);
            }
        }
        if unsafe { tailscale_set_control_url(handle, login_server.as_ptr()) } != 0 {
            let err = fail("set control URL");
            unsafe { tailscale_close(handle) };
            return Err(err);
        }
        tracing::info!(
            "embedded libtailscale starting Headscale {}",
            login_server.to_string_lossy()
        );
        if unsafe { tailscale_up(handle) } != 0 {
            let err = fail("libtailscale Headscale join failed");
            unsafe { tailscale_close(handle) };
            return Err(err);
        }
        let mut addr = vec![0i8; 128];
        let mut proxy = vec![0i8; 64];
        let mut local_api = vec![0i8; 64];
        if unsafe {
            tailscale_loopback(
                handle,
                addr.as_mut_ptr(),
                addr.len(),
                proxy.as_mut_ptr(),
                local_api.as_mut_ptr(),
            )
        } != 0
        {
            let err = fail("libtailscale loopback SOCKS failed");
            unsafe { tailscale_close(handle) };
            return Err(err);
        }
        let addr = unsafe { CStr::from_ptr(addr.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let proxy = unsafe { CStr::from_ptr(proxy.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let local_api_cred = unsafe { CStr::from_ptr(local_api.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let mut ips = vec![0i8; 128];
        let overlay_ip = if unsafe { tailscale_getips(handle, ips.as_mut_ptr(), ips.len()) } == 0 {
            unsafe { CStr::from_ptr(ips.as_ptr()) }
                .to_string_lossy()
                .split(',')
                .find(|ip| !ip.is_empty() && !ip.contains(':'))
                .map(str::to_string)
        } else {
            None
        };
        Ok((
            Arc::new(LibtailscaleSession {
                handle,
                closed: AtomicBool::new(false),
            }),
            addr,
            proxy,
            local_api_cred,
            overlay_ip,
        ))
    })
    .await
    .context("embedded libtailscale worker stopped unexpectedly")?
}

#[cfg(target_os = "windows")]
async fn start_socks_bridge(
    socks_addr: &str,
    proxy_cred: &str,
    target: &str,
) -> Result<Arc<SocksBridge>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let bridge = {
        let (shutdown, mut shutdown_rx) = watch::channel(false);
        let bridge = Arc::new(SocksBridge {
            addr: listener.local_addr()?,
            shutdown,
        });
        let socks_addr = socks_addr.to_string();
        let proxy_cred = proxy_cred.to_string();
        let target = target.to_string();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    changed = shutdown_rx.changed() => {
                        let _ = changed;
                        break;
                    }
                    accepted = listener.accept() => accepted,
                };
                let Ok((client, _)) = accepted else {
                    break;
                };
                let socks_addr = socks_addr.clone();
                let proxy_cred = proxy_cred.clone();
                let target = target.clone();
                let mut connection_shutdown = shutdown_rx.clone();
                tokio::spawn(async move {
                    tokio::select! {
                        result = proxy_socks5(client, &socks_addr, &proxy_cred, &target) => {
                            if let Err(err) = result {
                                tracing::debug!("Tailscale SOCKS bridge connection failed: {err}");
                            }
                        }
                        changed = connection_shutdown.changed() => {
                            let _ = changed;
                        }
                    }
                });
            }
        });
        bridge
    };
    Ok(bridge)
}

#[cfg(target_os = "windows")]
fn socks5_target_parts(target: &str) -> Result<(String, u16)> {
    let target = normalize_nodepool_endpoint(target);
    let (host, port) = if let Some(rest) = target.strip_prefix('[') {
        let (host, port) = rest
            .split_once(']')
            .ok_or_else(|| anyhow::anyhow!("invalid nodepool endpoint: {target}"))?;
        let port = port
            .strip_prefix(':')
            .ok_or_else(|| anyhow::anyhow!("invalid nodepool endpoint: {target}"))?;
        (host.to_string(), port)
    } else {
        let (host, port) = target
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid nodepool endpoint: {target}"))?;
        (host.to_string(), port)
    };
    Ok((host, port.parse()?))
}

#[cfg(target_os = "windows")]
async fn proxy_socks5(
    mut client: TcpStream,
    socks_addr: &str,
    proxy_cred: &str,
    target: &str,
) -> Result<()> {
    let mut proxy = TcpStream::connect(socks_addr).await?;
    proxy.write_all(&[5, 1, 2]).await?;
    let mut greeting = [0u8; 2];
    proxy.read_exact(&mut greeting).await?;
    if greeting != [5, 2] {
        bail!("libtailscale SOCKS5 proxy rejected username/password negotiation");
    }
    let username = b"tsnet";
    let password = proxy_cred.as_bytes();
    if password.len() > 255 {
        bail!("invalid libtailscale SOCKS credential");
    }
    proxy.write_all(&[1, username.len() as u8]).await?;
    proxy.write_all(username).await?;
    proxy.write_all(&[password.len() as u8]).await?;
    proxy.write_all(password).await?;
    let mut auth_response = [0u8; 2];
    proxy.read_exact(&mut auth_response).await?;
    if auth_response != [1, 0] {
        bail!("libtailscale SOCKS5 authentication failed");
    }
    let (host, port) = socks5_target_parts(target)?;
    let ip = host.parse::<IpAddr>();
    let mut request = vec![5, 1, 0];
    match ip {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            request.push(3);
            request.push(host.len().try_into()?);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    proxy.write_all(&request).await?;
    let mut response = [0u8; 4];
    proxy.read_exact(&mut response).await?;
    if response[1] != 0 {
        bail!("SOCKS5 proxy failed to connect to {target}");
    }
    let address_len = match response[3] {
        1 => 4,
        3 => {
            let mut len = [0u8; 1];
            proxy.read_exact(&mut len).await?;
            usize::from(len[0])
        }
        4 => 16,
        _ => bail!("invalid SOCKS5 address type"),
    };
    let mut discard = vec![0u8; address_len + 2];
    proxy.read_exact(&mut discard).await?;
    let _ = tokio::io::copy_bidirectional(&mut client, &mut proxy).await?;
    Ok(())
}

/// Parse WireGuard auth key format: wg-<private_key_hex>:<peer_public_key_hex>:<endpoint>
#[allow(dead_code)]
fn parse_wireguard_auth_key(
    auth_key: &str,
) -> Result<(
    boringtun::x25519::StaticSecret,
    boringtun::x25519::PublicKey,
    SocketAddr,
    Vec<Ipv4Addr>,
)> {
    let key_part = auth_key.strip_prefix("wg-").unwrap_or(auth_key);
    let parts: Vec<&str> = key_part.split(':').collect();
    if parts.len() < 3 {
        bail!("Invalid WireGuard auth key format: expected wg-privkey:peerpubkey:endpoint");
    }

    let priv_bytes = hex::decode(parts[0])?;
    let peer_bytes = hex::decode(parts[1])?;
    let endpoint: SocketAddr = parts[2].parse()?;

    if priv_bytes.len() != 32 || peer_bytes.len() != 32 {
        bail!("WireGuard keys must be 32 bytes each");
    }

    let mut priv_arr = [0u8; 32];
    let mut peer_arr = [0u8; 32];
    priv_arr.copy_from_slice(&priv_bytes);
    peer_arr.copy_from_slice(&peer_bytes);

    let private_key = boringtun::x25519::StaticSecret::from(priv_arr);
    let peer_public_key = boringtun::x25519::PublicKey::from(peer_arr);
    let allowed_ips = vec!["100.64.0.0".parse()?, "100.64.0.1".parse()?];

    Ok((private_key, peer_public_key, endpoint, allowed_ips))
}

/// Ping nodepool peer over WireGuard tunnel
#[allow(dead_code)]
async fn ping_nodepool_over_wireguard(session: &VpnSession) -> Result<bool> {
    if session.transport != VpnTransport::Wireguard {
        return Ok(false);
    }

    if let Some(wg_tunnel) = &session.wg_tunnel {
        let tunnel = wg_tunnel.lock().await;
        // Send a simple ICMP-like packet through the tunnel
        // For now, just check if tunnel is connected
        Ok(tunnel.is_connected().await)
    } else {
        Ok(false)
    }
}

fn nodepool_http_endpoint(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.is_empty() {
        return None;
    }
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Some(endpoint.to_string())
    } else {
        Some(format!("http://{endpoint}"))
    }
}

/// Check whether a nodepool endpoint completes the same HTTP/2 transport
/// handshake used by the tonic clients. A TCP-open but non-gRPC listener is not
/// considered ready.
async fn nodepool_endpoint_reachable(endpoint: &str) -> bool {
    let Some(endpoint) = nodepool_http_endpoint(endpoint) else {
        return false;
    };
    let Ok(endpoint) = Endpoint::from_shared(endpoint) else {
        return false;
    };
    let endpoint = endpoint.connect_timeout(NODEPOOL_PROBE_TIMEOUT);
    let Ok(Ok(channel)) = tokio::time::timeout(NODEPOOL_PROBE_TIMEOUT, endpoint.connect()).await
    else {
        return false;
    };

    // `Endpoint::connect` only establishes the underlying socket. Send a small
    // unary gRPC request so an accept-and-stall listener cannot be reported ready.
    // The path is intentionally unknown to the application: an immediate gRPC
    // status (including UNIMPLEMENTED) still proves the HTTP/2 transport works.
    let mut grpc = Grpc::new(channel);
    if !tokio::time::timeout(NODEPOOL_PROBE_TIMEOUT, grpc.ready())
        .await
        .is_ok_and(|result| result.is_ok())
    {
        return false;
    }
    let probe = grpc.unary(
        Request::new(TransportProbeRequest {}),
        PathAndQuery::from_static("/hivemind.client_runtime.TransportProbe/Probe"),
        ProstCodec::<TransportProbeRequest, TransportProbeResponse>::default(),
    );
    match tokio::time::timeout(NODEPOOL_PROBE_TIMEOUT, probe).await {
        Ok(Ok(_)) => true,
        // A gRPC status means the HTTP/2 server answered. A tonic transport
        // error carries a source and must not count as Nodepool readiness.
        Ok(Err(status)) => status.source().is_none(),
        Err(_) => false,
    }
}

/// Extract host from endpoint string
#[cfg(test)]
fn endpoint_host(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    let endpoint = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    let host = if endpoint.starts_with('[') {
        endpoint
            .split(']')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
    } else {
        endpoint.split(':').next().unwrap_or("")
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// Get the VPN state directory for a role. A process can isolate its VPN
/// identity with `HIVEMIND_VPN_STATE_ROOT` without changing the default path.
fn vpn_state_dir(role: ClientRole) -> PathBuf {
    let root = std::env::var_os("HIVEMIND_VPN_STATE_ROOT")
        .filter(|root| !root.is_empty())
        .map(PathBuf::from);
    vpn_state_dir_with_root(role, root)
}

fn vpn_state_dir_with_root(role: ClientRole, root: Option<PathBuf>) -> PathBuf {
    let base = root
        .or_else(dirs::data_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".hivemind")
        .join(format!("{}-vpn", role.as_str()))
}

fn device_id_path(role: ClientRole) -> PathBuf {
    vpn_state_dir(role).join("device-id")
}

fn state_marker_path(role: ClientRole) -> PathBuf {
    vpn_state_dir(role).join("state-ready")
}

fn persisted_device_id(role: ClientRole) -> Result<String> {
    let state_dir = vpn_state_dir(role);
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("failed to create VPN state dir {}", state_dir.display()))?;
    let path = device_id_path(role);
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let existing = existing.trim();
        if !existing.is_empty()
            && existing.len() <= 64
            && existing.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Ok(existing.to_string());
        }
    }

    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let generated = hex::encode(bytes);
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, format!("{generated}\n")).with_context(|| {
        format!(
            "failed to write VPN device identity {}",
            temporary.display()
        )
    })?;
    std::fs::rename(&temporary, &path)
        .with_context(|| format!("failed to persist VPN device identity {}", path.display()))?;
    Ok(generated)
}

fn client_device_name(role: ClientRole) -> Result<String> {
    Ok(client_name_for_device(role, &persisted_device_id(role)?))
}

#[cfg(target_os = "windows")]
fn reset_libtailscale_state_for_new_auth_key(role: ClientRole) -> Result<()> {
    let state_dir = vpn_state_dir(role);
    for path in [state_dir.join("tailscaled.state"), state_marker_path(role)] {
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(
                "{} removed stale local VPN state {} before fresh auth-key join",
                role.as_str(),
                path.display()
            ),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed to remove stale {} VPN state {} before fresh auth-key join",
                        role.as_str(),
                        path.display()
                    )
                });
            }
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn reset_libtailscale_state_for_new_auth_key(_role: ClientRole) -> Result<()> {
    Ok(())
}

fn persisted_vpn_identity_matches(plan: &VpnReconnectPlan) -> bool {
    let Ok(marker) = std::fs::read(state_marker_path(plan.role)) else {
        return false;
    };
    let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&marker) else {
        return false;
    };
    marker_matches_reconnect_plan(&marker, plan)
}

fn marker_matches_reconnect_plan(marker: &serde_json::Value, plan: &VpnReconnectPlan) -> bool {
    matches!(marker.get("version").and_then(|v| v.as_u64()), Some(1 | 2))
        && marker.get("role").and_then(|v| v.as_str()) == Some(plan.role.as_str())
        && marker
            .get("login_server")
            .and_then(|v| v.as_str())
            .is_some_and(|server| server.trim_end_matches('/') == plan.login_server)
        && marker.get("hostname").and_then(|v| v.as_str()) == Some(plan.hostname.as_str())
}

fn persisted_nodepool_target(plan: &VpnReconnectPlan) -> Option<String> {
    let marker = std::fs::read(state_marker_path(plan.role)).ok()?;
    let marker: serde_json::Value = serde_json::from_slice(&marker).ok()?;
    persisted_target_from_marker(&marker, plan)
}

fn persisted_target_from_marker(
    marker: &serde_json::Value,
    plan: &VpnReconnectPlan,
) -> Option<String> {
    if plan.operator_endpoint
        || !marker_matches_reconnect_plan(marker, plan)
        || marker.get("version")?.as_u64()? != 2
    {
        return None;
    }
    validate_overlay_endpoint(marker.get("nodepool_target")?.as_str()?)
}

fn mark_persisted_vpn_state(
    role: ClientRole,
    login_server: &str,
    hostname: &str,
    target: &str,
) -> Result<()> {
    let state_dir = vpn_state_dir(role);
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("failed to create VPN state dir {}", state_dir.display()))?;
    let marker = serde_json::json!({
        "version": 2,
        "role": role.as_str(),
        "login_server": login_server,
        "hostname": hostname,
        "nodepool_target": validate_overlay_endpoint(target),
    });
    let path = state_marker_path(role);
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, serde_json::to_vec(&marker)?)
        .with_context(|| format!("failed to write VPN state marker {}", temporary.display()))?;
    std::fs::rename(&temporary, &path)
        .with_context(|| format!("failed to persist VPN state marker {}", path.display()))?;
    Ok(())
}

/// Generate a short host identifier
fn short_host_id() -> String {
    // Use a hash of the hostname or a random short ID
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    hostname.hash(&mut hasher);
    format!("{:x}", hasher.finish())[..8].to_string()
}

/// Sanitize hostname to be a valid DNS label
fn sanitize_hostname(hostname: &str) -> String {
    hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn bounded_hostname(hostname: &str) -> String {
    const MAX_HOSTNAME_LEN: usize = 63;
    let sanitized = sanitize_hostname(hostname);
    let bounded = sanitized.chars().take(MAX_HOSTNAME_LEN).collect::<String>();
    let bounded = bounded.trim_matches('-').to_string();
    if bounded.is_empty() {
        "hivemind-node".to_string()
    } else {
        bounded
    }
}

fn client_name_for_device(role: ClientRole, device_id: &str) -> String {
    let device_id = sanitize_hostname(device_id);
    let prefix = format!("hivemind-{}-", role.as_str());
    let max_device_len = 48usize.saturating_sub(prefix.len());
    let device_id = device_id.chars().take(max_device_len).collect::<String>();
    format!("{prefix}{device_id}")
}

/// Check if an environment variable is truthy
fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .map(|v| {
            let v = v.trim().to_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false)
}

/// Get environment variable trimmed
fn env_trim(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Get first non-empty value from a list of options
fn first_nonempty(options: &[Option<String>]) -> Option<String> {
    for val in options.iter().flatten() {
        if !val.is_empty() {
            return Some(val.clone());
        }
    }
    None
}

/// Normalize HTTP base URL
fn normalize_http_base(base: &str) -> String {
    let base = base.trim();
    if base.starts_with("http://") || base.starts_with("https://") {
        base.trim_end_matches('/').to_string()
    } else {
        format!("https://{}", base.trim_end_matches('/'))
    }
}

// WireGuard implementation using boringtun
mod wireguard {
    use super::*;
    use boringtun::noise::{Tunn, TunnResult};
    use boringtun::x25519::{PublicKey, StaticSecret};
    use rand::rngs::OsRng;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::net::UdpSocket;
    use tokio::sync::Mutex as TokioMutex;
    use tokio::time::interval;

    /// WireGuard peer configuration
    #[derive(Clone)]
    pub struct WireguardPeerConfig {
        pub public_key: PublicKey,
        pub endpoint: SocketAddr,
        pub allowed_ips: Vec<Ipv4Addr>,
        pub persistent_keepalive: Option<u16>,
    }

    /// WireGuard interface configuration
    #[derive(Clone)]
    pub struct WireguardConfig {
        pub private_key: StaticSecret,
        pub listen_port: u16,
        pub peers: Vec<WireguardPeerConfig>,
        pub mtu: usize,
    }

    impl WireguardConfig {
        /// Create a new WireGuard config for connecting to nodepool
        pub fn for_nodepool(
            private_key: StaticSecret,
            peer_public_key: PublicKey,
            endpoint: SocketAddr,
            allowed_ips: Vec<Ipv4Addr>,
        ) -> Self {
            Self {
                private_key,
                listen_port: 0, // Let OS assign
                peers: vec![WireguardPeerConfig {
                    public_key: peer_public_key,
                    endpoint,
                    allowed_ips,
                    persistent_keepalive: Some(25),
                }],
                mtu: 1420,
            }
        }

        /// Generate a random private key
        pub fn generate_private_key() -> StaticSecret {
            StaticSecret::random_from_rng(OsRng)
        }

        /// Get the public key from a private key
        pub fn public_key(private_key: &StaticSecret) -> PublicKey {
            PublicKey::from(private_key)
        }
    }

    /// WireGuard tunnel state for managing the connection
    pub struct WireguardTunnel {
        config: WireguardConfig,
        tunnel: Arc<TokioMutex<Tunn>>,
        socket: Arc<UdpSocket>,
        local_addr: SocketAddr,
        last_handshake: Arc<TokioMutex<Option<Instant>>>,
        running: Arc<TokioMutex<bool>>,
    }

    impl WireguardTunnel {
        /// Create and start a new WireGuard tunnel
        pub async fn new(config: WireguardConfig) -> Result<Self> {
            // Create UDP socket
            let socket = UdpSocket::bind(("0.0.0.0", config.listen_port))
                .await
                .context("Failed to bind UDP socket for WireGuard")?;
            let local_addr = socket
                .local_addr()
                .context("Failed to get local socket address")?;
            let socket = Arc::new(socket);

            // Create Tunn (boringtun's WireGuard implementation)
            // Clone values since we need to move config into the struct later
            let private_key = config.private_key.clone();
            let peer_public_key = config.peers[0].public_key;
            let persistent_keepalive = config.peers[0].persistent_keepalive;
            let tunnel = Tunn::new(
                private_key,
                peer_public_key,
                None, // No preshared key
                persistent_keepalive,
                0,    // index
                None, // rate_limiter
            );
            let tunnel = WireguardTunnel {
                config,
                tunnel: Arc::new(TokioMutex::new(tunnel)),
                socket,
                local_addr,
                last_handshake: Arc::new(TokioMutex::new(None)),
                running: Arc::new(TokioMutex::new(true)),
            };

            // Start the packet processing loop
            tunnel.start_packet_loop().await?;

            Ok(tunnel)
        }

        /// Start the packet processing loop
        async fn start_packet_loop(&self) -> Result<()> {
            let socket = self.socket.clone();
            let tunnel = self.tunnel.clone();
            let running = self.running.clone();
            let last_handshake = self.last_handshake.clone();
            let peer_endpoint = self.config.peers[0].endpoint;

            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let mut interval = interval(Duration::from_millis(100));

                loop {
                    // Check if still running
                    if !*running.lock().await {
                        break;
                    }

                    // Process pending tunnel events
                    tokio::select! {
                        _ = interval.tick() => {
                            // Update timers and generate packets to send
                            let mut out_buf = [0u8; 2048];
                            let mut tunnel_guard = tunnel.lock().await;
                            match tunnel_guard.update_timers(&mut out_buf) {
                                TunnResult::WriteToNetwork(buf) => {
                                    if !buf.is_empty() {
                                        if let Err(e) = socket.send_to(buf, peer_endpoint).await {
                                            tracing::debug!("WireGuard send error: {:?}", e);
                                        }
                                    }
                                }
                                TunnResult::WriteToTunnelV4(_, _) |
                                TunnResult::WriteToTunnelV6(_, _) => {
                                    // Packets for TUN interface - not used in our case
                                }
                                TunnResult::Done => {}
                                TunnResult::Err(e) => {
                                    tracing::debug!("WireGuard timer error: {:?}", e);
                                }
                            }

                            // Check handshake status
                            if tunnel_guard.time_since_last_handshake().is_some() {
                                *last_handshake.lock().await = Some(Instant::now());
                            }
                        }
                        // Receive packets from network
                        result = socket.recv_from(&mut buf) => {
                            match result {
                                Ok((n, src)) => {
                                    if src == peer_endpoint {
                                        let mut tunnel_guard = tunnel.lock().await;
                                        // Parse incoming packet
                                        match Tunn::parse_incoming_packet(&buf[..n]) {
                                            Ok(_packet) => {
                                                let mut out_buf = [0u8; 2048];
                                                match tunnel_guard.decapsulate(None, &buf[..n], &mut out_buf) {
                                                    TunnResult::WriteToNetwork(buf) => {
                                                        if !buf.is_empty() {
                                                            if let Err(e) = socket.send_to(buf, peer_endpoint).await {
                                                                tracing::debug!("WireGuard response send error: {:?}", e);
                                                            }
                                                        }
                                                    }
                                                    TunnResult::WriteToTunnelV4(_, _) |
                                                    TunnResult::WriteToTunnelV6(_, _) => {
                                                        // Decrypted packet for TUN
                                                    }
                                                    TunnResult::Done => {}
                                                    TunnResult::Err(e) => {
                                                        tracing::debug!("WireGuard decapsulate error: {:?}", e);
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                tracing::debug!("WireGuard parse_incoming_packet error: {:?}", e);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    if e.kind() != std::io::ErrorKind::WouldBlock {
                                        tracing::debug!("WireGuard recv error: {:?}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            });

            Ok(())
        }

        /// Check if the tunnel is connected (handshake completed)
        pub async fn is_connected(&self) -> bool {
            // Check if we have a recent handshake
            let tunnel = self.tunnel.lock().await;
            if let Some(duration) = tunnel.time_since_last_handshake() {
                duration < Duration::from_secs(180) // 3 minutes
            } else {
                false
            }
        }

        /// Get the local address of the tunnel
        pub fn local_addr(&self) -> SocketAddr {
            self.local_addr
        }

        /// Stop the tunnel
        pub async fn stop(&self) {
            *self.running.lock().await = false;
        }
    }

    /// Build WireGuard configuration from VPN config provided by website-api
    #[allow(dead_code)]
    pub async fn build_wireguard_config_from_vpn(
        _vpn_config: &WebsiteVpnConfigResponse,
        nodepool_endpoint: &str,
    ) -> Result<(StaticSecret, PublicKey, SocketAddr, Vec<Ipv4Addr>)> {
        // Parse the nodepool endpoint
        let endpoint: SocketAddr = nodepool_endpoint
            .parse()
            .context("Invalid nodepool endpoint")?;

        // For WireGuard, we need the peer's public key. This would typically come from
        // the VPN config or be derived from the Headscale/Nodepool setup.
        // Since website-api doesn't directly provide WireGuard keys, we need to either:
        // 1. Have website-api return WireGuard peer public key
        // 2. Use a well-known platform public key
        // 3. Derive from the auth_key (not cryptographically sound, but for compatibility)

        // Use a platform-known public key for nodepool
        // Priority: 1) HIVEMIND_WG_PLATFORM_PUBLIC_KEY env var, 2) Default platform key constant
        let platform_public_key = if let Ok(key) = std::env::var("HIVEMIND_WG_PLATFORM_PUBLIC_KEY")
        {
            // Parse from hex (env var takes precedence)
            let bytes = hex::decode(key.trim())?;
            if bytes.len() != 32 {
                bail!("Platform public key must be 32 bytes");
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            PublicKey::from(arr)
        } else if !super::DEFAULT_PLATFORM_WG_PUBLIC_KEY.is_empty() {
            // Use default platform public key constant
            let bytes = hex::decode(super::DEFAULT_PLATFORM_WG_PUBLIC_KEY)?;
            if bytes.len() != 32 {
                bail!("Default platform public key must be 32 bytes");
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            PublicKey::from(arr)
        } else {
            bail!("WireGuard platform public key not configured. Set HIVEMIND_WG_PLATFORM_PUBLIC_KEY environment variable or update DEFAULT_PLATFORM_WG_PUBLIC_KEY constant with the nodepool's WireGuard public key (32-byte hex-encoded X25519 public key).")
        };

        // Generate our private key
        let private_key = WireguardConfig::generate_private_key();

        // Allowed IPs - the VPN subnet (100.64.0.0/10 for Tailscale compatibility)
        let allowed_ips = vec![
            "100.64.0.0".parse()?,
            "100.64.0.1".parse()?, // nodepool
        ];

        Ok((private_key, platform_public_key, endpoint, allowed_ips))
    }

    /// Parse WireGuard config from the config_text returned by website-api
    /// The config_text may contain WireGuard-specific fields like:
    /// # wireguard_private_key=...
    /// # wireguard_peer_public_key=...
    /// # wireguard_endpoint=...
    /// # wireguard_allowed_ips=...
    #[allow(dead_code)]
    pub fn parse_wireguard_from_config_text(
        config_text: &str,
    ) -> Option<(StaticSecret, PublicKey, SocketAddr, Vec<Ipv4Addr>)> {
        let mut private_key: Option<StaticSecret> = None;
        let mut peer_public_key: Option<PublicKey> = None;
        let mut endpoint: Option<SocketAddr> = None;
        let mut allowed_ips: Vec<Ipv4Addr> = Vec::new();

        for line in config_text.lines() {
            let line = line.trim();
            if let Some(val) = line.strip_prefix("# wireguard_private_key=") {
                if let Ok(bytes) = hex::decode(val.trim()) {
                    if bytes.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&bytes);
                        private_key = Some(StaticSecret::from(arr));
                    }
                }
            } else if let Some(val) = line.strip_prefix("# wireguard_peer_public_key=") {
                if let Ok(bytes) = hex::decode(val.trim()) {
                    if bytes.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&bytes);
                        peer_public_key = Some(PublicKey::from(arr));
                    }
                }
            } else if let Some(val) = line.strip_prefix("# wireguard_endpoint=") {
                if let Ok(addr) = val.trim().parse() {
                    endpoint = Some(addr);
                }
            } else if let Some(val) = line.strip_prefix("# wireguard_allowed_ips=") {
                for ip_str in val.trim().split(',') {
                    if let Ok(ip) = ip_str.trim().parse() {
                        allowed_ips.push(ip);
                    }
                }
            }
        }

        if let (Some(priv_key), Some(pub_key), Some(ep)) = (private_key, peer_public_key, endpoint)
        {
            if allowed_ips.is_empty() {
                allowed_ips = vec!["100.64.0.0".parse().unwrap(), "100.64.0.1".parse().unwrap()];
            }
            Some((priv_key, pub_key, ep, allowed_ips))
        } else {
            None
        }
    }
}

/// Only accept online overlay peers whose Headscale identity matches Nodepool.
#[cfg(any(test, target_os = "windows"))]
fn extract_nodepool_peer_ips(status: &serde_json::Value, hostnames: &[String]) -> Vec<String> {
    let mut ips = Vec::new();
    if let Some(peer_map) = status.get("Peer").and_then(|v| v.as_object()) {
        for peer_info in peer_map.values() {
            if peer_info.get("Online").and_then(|v| v.as_bool()) != Some(true) {
                continue;
            }
            let hostname_match = peer_info
                .get("HostName")
                .and_then(|v| v.as_str())
                .is_some_and(|name| hostnames.iter().any(|host| name.eq_ignore_ascii_case(host)));
            let dns_name_match = peer_info
                .get("DNSName")
                .and_then(|v| v.as_str())
                .is_some_and(|name| {
                    hostnames.iter().any(|host| {
                        name.eq_ignore_ascii_case(host)
                            || name
                                .to_ascii_lowercase()
                                .starts_with(&format!("{}.", host.to_ascii_lowercase()))
                    })
                });
            if !(hostname_match || dns_name_match) {
                continue;
            }
            if let Some(tailscale_ips) = peer_info.get("TailscaleIPs").and_then(|v| v.as_array()) {
                for ip in tailscale_ips.iter().filter_map(|value| value.as_str()) {
                    let candidate = format!("{ip}:{DEFAULT_NODEPOOL_GRPC_PORT}");
                    if ip.parse::<Ipv4Addr>().is_ok()
                        && validate_overlay_endpoint(&candidate).is_some()
                        && !ips.iter().any(|existing| existing == ip)
                    {
                        ips.push(ip.to_string());
                    }
                }
            }
        }
    }
    ips
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::OnceLock;

    /// Serializes tests that mutate process environment variables. Cargo runs
    /// the tests in one binary on parallel threads, and two of these tests
    /// toggle `HIVEMIND_DISABLE_WEBSITE_VPN` in opposite directions, which
    /// previously raced.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn runtime_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn clear_runtime_for_test(role: ClientRole) {
        runtimes_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&role);
    }

    fn test_reconnect_plan() -> VpnReconnectPlan {
        VpnReconnectPlan::new(
            ClientRole::Worker,
            Some("test-auth-key"),
            "https://headscale.example",
            "worker-test",
            "100.64.0.1:50051",
            Some("127.0.0.1:50053"),
            Duration::from_secs(1),
            true,
        )
    }

    fn test_vpn_session() -> VpnSession {
        VpnSession {
            role: ClientRole::Worker,
            transport: VpnTransport::Tailscale,
            state_dir: PathBuf::from("test-vpn-state"),
            bridge_addr: Some("127.0.0.1:50051".parse().unwrap()),
            overlay_ip: Some("100.64.0.20".into()),
            auth_key: "test-auth-key".into(),
            login_server: "https://headscale.example".into(),
            hostname: "worker-test".into(),
            nodepool_target: "100.64.0.1:50051".into(),
            worker_grpc_port: 50053,
            #[cfg(target_os = "windows")]
            userspace_socks_addr: None,
            #[cfg(target_os = "windows")]
            userspace_proxy_cred: None,
            #[cfg(target_os = "windows")]
            local_api_cred: None,
            #[cfg(target_os = "windows")]
            active_bridge: StdMutex::new(None),
            #[cfg(target_os = "windows")]
            additional_bridges: StdMutex::new(HashMap::new()),
            wg_private_key: None,
            wg_peer_public_key: None,
            wg_endpoint: None,
            wg_allowed_ips: None,
            wg_tunnel: None,
            #[cfg(target_os = "windows")]
            libtailscale: None,
        }
    }

    #[test]
    fn vpn_state_root_isolates_processes_and_preserves_role_directories() {
        let first = vpn_state_dir_with_root(ClientRole::Worker, Some(PathBuf::from("first")));
        let second = vpn_state_dir_with_root(ClientRole::Worker, Some(PathBuf::from("second")));
        assert_eq!(first, PathBuf::from("first/.hivemind/worker-vpn"));
        assert_eq!(second, PathBuf::from("second/.hivemind/worker-vpn"));
        assert_ne!(first, second);
        assert_eq!(
            vpn_state_dir_with_root(ClientRole::Master, Some(PathBuf::from("first"))),
            PathBuf::from("first/.hivemind/master-vpn")
        );
        assert_eq!(
            vpn_state_dir_with_root(ClientRole::Worker, None),
            dirs::data_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".hivemind/worker-vpn")
        );
    }

    #[test]
    fn plan_skips_without_bootstrap_settings() {
        let plan = plan_vpn_bootstrap(
            None,
            None,
            None,
            Some("http://localhost:8080"),
            ClientRole::Master,
        )
        .unwrap();
        assert_eq!(plan, VpnBootstrapPlan::Skip);
    }

    #[test]
    fn plan_skips_when_only_login_server_configured() {
        let plan = plan_vpn_bootstrap(
            None,
            Some("https://Headscale.justin0711.com"),
            None,
            Some("https://Headscale.justin0711.com"),
            ClientRole::Worker,
        )
        .unwrap();
        assert_eq!(plan, VpnBootstrapPlan::Skip);
    }

    #[test]
    fn plan_requires_login_server_when_authkey_configured() {
        let err = plan_vpn_bootstrap(
            Some("tskey-auth-test"),
            None,
            None,
            None,
            ClientRole::Master,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("MASTER_VPN_LOGIN_SERVER"), "{err}");
    }

    #[test]
    fn plan_uses_config_login_server_fallback_with_authkey() {
        let plan = plan_vpn_bootstrap(
            Some("tskey-auth-test"),
            None,
            Some("master-demo"),
            Some("http://headscale.example"),
            ClientRole::Master,
        )
        .unwrap();
        assert_eq!(
            plan,
            VpnBootstrapPlan::Join {
                auth_key: "tskey-auth-test".into(),
                login_server: "http://headscale.example".into(),
                hostname: "master-demo".into(),
            }
        );
    }

    #[test]
    fn website_api_base_defaults_to_official_endpoint() {
        let _env = env_lock();
        let config = HivemindConfig::default();
        // Ensure disable flags are unset for this process.
        let orig_disable_vpn = std::env::var("HIVEMIND_DISABLE_WEBSITE_VPN").ok();
        let orig_master_disable = std::env::var("MASTER_DISABLE_WEBSITE_VPN").ok();
        let orig_master_base = std::env::var("MASTER_WEBSITE_API_BASE").ok();
        let orig_base = std::env::var("WEBSITE_API_BASE").ok();
        let orig_hivemind_base = std::env::var("HIVEMIND_WEBSITE_API_BASE").ok();
        std::env::remove_var("HIVEMIND_DISABLE_WEBSITE_VPN");
        std::env::remove_var("MASTER_DISABLE_WEBSITE_VPN");
        std::env::remove_var("MASTER_WEBSITE_API_BASE");
        std::env::remove_var("WEBSITE_API_BASE");
        std::env::remove_var("HIVEMIND_WEBSITE_API_BASE");

        let base = website_api_base(&config, ClientRole::Master).unwrap();
        assert_eq!(base, DEFAULT_WEBSITE_API_BASE);

        // Restore original values
        if let Some(v) = orig_disable_vpn {
            std::env::set_var("HIVEMIND_DISABLE_WEBSITE_VPN", v);
        }
        if let Some(v) = orig_master_disable {
            std::env::set_var("MASTER_DISABLE_WEBSITE_VPN", v);
        }
        if let Some(v) = orig_master_base {
            std::env::set_var("MASTER_WEBSITE_API_BASE", v);
        }
        if let Some(v) = orig_base {
            std::env::set_var("WEBSITE_API_BASE", v);
        }
        if let Some(v) = orig_hivemind_base {
            std::env::set_var("HIVEMIND_WEBSITE_API_BASE", v);
        }
    }

    #[test]
    fn website_api_base_can_be_disabled() {
        let _env = env_lock();
        let config = HivemindConfig::default();
        let orig_disable_vpn = std::env::var("HIVEMIND_DISABLE_WEBSITE_VPN").ok();
        std::env::set_var("HIVEMIND_DISABLE_WEBSITE_VPN", "1");
        let base = website_api_base(&config, ClientRole::Worker);
        // Restore original value
        if let Some(v) = orig_disable_vpn {
            std::env::set_var("HIVEMIND_DISABLE_WEBSITE_VPN", v);
        } else {
            std::env::remove_var("HIVEMIND_DISABLE_WEBSITE_VPN");
        }
        assert!(base.is_none());
    }

    #[test]
    fn strict_bootstrap_status_requires_an_overlay_address() {
        let mut config = HivemindConfig::for_test();
        config.vpn.external_overlay_mode = ExternalOverlayMode::Strict;
        let status = VpnBootstrapStatus {
            state: VpnBootstrapState::Ready,
            endpoint: Some("127.0.0.1:50051".into()),
            overlay_ip: None,
            message: None,
        };
        assert!(!vpn_bootstrap_status_success(
            &config,
            ClientRole::Worker,
            &status
        ));

        let status = VpnBootstrapStatus {
            overlay_ip: Some("100.64.0.20".into()),
            ..status
        };
        assert!(vpn_bootstrap_status_success(
            &config,
            ClientRole::Worker,
            &status
        ));
    }

    #[tokio::test]
    async fn strict_endpoint_candidates_never_return_a_direct_endpoint() {
        let candidates =
            nodepool_endpoint_candidates(ClientRole::Worker, "nodepool:50051", None, true).await;
        assert!(candidates.is_empty());
    }

    #[test]
    fn strict_configuration_rejects_disabled_website_enrollment() {
        let _env = env_lock();
        let mut config = HivemindConfig::for_test();
        config.vpn.external_overlay_mode = ExternalOverlayMode::Strict;
        let original = std::env::var_os("HIVEMIND_DISABLE_WEBSITE_VPN");
        std::env::set_var("HIVEMIND_DISABLE_WEBSITE_VPN", "1");
        let result = validate_external_overlay_configuration(&config, ClientRole::Worker);
        match original {
            Some(value) => std::env::set_var("HIVEMIND_DISABLE_WEBSITE_VPN", value),
            None => std::env::remove_var("HIVEMIND_DISABLE_WEBSITE_VPN"),
        }
        let error = result.unwrap_err().to_string();
        let expected = if cfg!(target_os = "windows") {
            "rejects disabled Website API enrollment"
        } else {
            "strict external overlay requires a native Windows client"
        };
        assert!(error.contains(expected), "{error}");
    }

    #[test]
    fn local_ui_url_rewrites_unspecified_bind_addresses() {
        assert_eq!(local_ui_url("0.0.0.0:8082"), "http://127.0.0.1:8082/");
        assert_eq!(local_ui_url("127.0.0.1:18080"), "http://127.0.0.1:18080/");
        assert_eq!(local_ui_url("[::]:8082"), "http://127.0.0.1:8082/");
    }

    #[test]
    fn local_browser_url_uses_the_bound_address_and_valid_ipv6_brackets() {
        assert_eq!(
            local_browser_url("0.0.0.0:18080".parse().unwrap()),
            "http://127.0.0.1:18080/"
        );
        assert_eq!(
            local_browser_url("[::]:18080".parse().unwrap()),
            "http://[::1]:18080/"
        );
        assert_eq!(
            local_browser_url("[::1]:18080".parse().unwrap()),
            "http://[::1]:18080/"
        );
    }

    #[test]
    fn local_webview_requires_local_ipv4_listener_and_bundled_ui() {
        let local: SocketAddr = "127.0.0.1:18080".parse().unwrap();
        let wildcard: SocketAddr = "0.0.0.0:18081".parse().unwrap();
        let remote: SocketAddr = "192.0.2.1:18080".parse().unwrap();
        let alternate_loopback: SocketAddr = "127.0.0.2:18080".parse().unwrap();
        let ipv6: SocketAddr = "[::1]:18080".parse().unwrap();
        let unbound: SocketAddr = "127.0.0.1:0".parse().unwrap();

        assert_eq!(
            local_webview_url(local, true, false).as_deref(),
            Some("http://127.0.0.1:18080/")
        );
        assert_eq!(
            local_webview_url(wildcard, true, false).as_deref(),
            Some("http://127.0.0.1:18081/")
        );
        for addr in [local, remote, ipv6, unbound] {
            assert!(local_webview_url(addr, false, false).is_none());
        }
        for addr in [remote, alternate_loopback, ipv6, unbound] {
            assert!(local_webview_url(addr, true, false).is_none());
        }
        assert!(local_webview_url(local, true, true).is_none());
    }

    #[test]
    fn webview_helper_executables_are_role_scoped() {
        assert_eq!(
            ClientUiRole::Master.windows_executables(),
            ("hivemind-master.exe", "hivemind-master-ui.exe")
        );
        assert_eq!(
            ClientUiRole::Worker.windows_executables(),
            ("hivemind-worker.exe", "hivemind-worker-ui.exe")
        );
    }

    #[test]
    fn webview_helper_environment_is_allowlisted() {
        assert_eq!(
            LOCAL_UI_ENV_ALLOWLIST,
            &[
                "SystemRoot",
                "WINDIR",
                "USERPROFILE",
                "LOCALAPPDATA",
                "APPDATA",
                "TEMP",
                "TMP",
                "PATH",
                "ProgramFiles",
                "ProgramFiles(x86)",
            ]
        );
        let environment = [
            ("systemroot", "C:\\Windows"),
            ("WINDIR", "C:\\Windows"),
            ("USERPROFILE", "C:\\Users\\user"),
            ("LOCALAPPDATA", "C:\\Users\\user\\AppData\\Local"),
            ("APPDATA", "C:\\Users\\user\\AppData\\Roaming"),
            ("TEMP", "C:\\Temp"),
            ("TMP", "C:\\Temp"),
            ("PATH", "C:\\Windows\\System32"),
            ("ProgramFiles", "C:\\Program Files"),
            ("ProgramFiles(x86)", "C:\\Program Files (x86)"),
            ("HIVEMIND_CONFIG", "C:\\private\\config.json"),
            ("JWT_SECRET", "must-not-leak"),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()));

        let filtered = filtered_local_ui_environment(environment).collect::<Vec<_>>();
        assert_eq!(filtered.len(), LOCAL_UI_ENV_ALLOWLIST.len());
        assert_eq!(filtered[0], ("systemroot".into(), "C:\\Windows".into()));
        assert!(!filtered.iter().any(|(name, _)| {
            name.to_string_lossy()
                .eq_ignore_ascii_case("HIVEMIND_CONFIG")
                || name.to_string_lossy().eq_ignore_ascii_case("JWT_SECRET")
        }));
    }

    #[tokio::test]
    async fn webview_readiness_requires_marker_and_times_out() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        writer.write_all(LOCAL_UI_READY_MARKER).await.unwrap();
        await_local_ui_readiness(&mut reader, Duration::from_millis(100))
            .await
            .unwrap();

        let (mut reader, mut writer) = tokio::io::duplex(64);
        writer.write_all(b"NOT_READY\\n").await.unwrap();
        assert!(
            await_local_ui_readiness(&mut reader, Duration::from_millis(100))
                .await
                .is_err()
        );

        let (mut reader, _writer) = tokio::io::duplex(64);
        assert!(
            await_local_ui_readiness(&mut reader, Duration::from_millis(10))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unavailable_webview_falls_back_to_browser() {
        let attempted_webview = Arc::new(StdMutex::new(false));
        let browser_urls = Arc::new(StdMutex::new(Vec::new()));
        let attempted_webview_for_launch = Arc::clone(&attempted_webview);
        let browser_urls_for_open = Arc::clone(&browser_urls);

        open_ui_with_browser_fallback(
            ClientUiRole::Worker,
            Some("http://127.0.0.1:18080/"),
            "http://127.0.0.1:18080/",
            move |_| {
                *attempted_webview_for_launch
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
                async { Err(anyhow::anyhow!("WebView2 Runtime is unavailable")) }
            },
            move |url| {
                browser_urls_for_open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(url.to_owned());
                Ok(())
            },
        )
        .await;

        assert!(*attempted_webview.lock().unwrap());
        assert_eq!(*browser_urls.lock().unwrap(), ["http://127.0.0.1:18080/"]);
    }

    #[tokio::test]
    async fn successful_webview_skips_browser_and_ineligible_webview_uses_browser() {
        let browser_urls = Arc::new(StdMutex::new(Vec::new()));
        let browser_urls_for_open = Arc::clone(&browser_urls);
        open_ui_with_browser_fallback(
            ClientUiRole::Master,
            Some("http://127.0.0.1:8082/"),
            "http://127.0.0.1:8082/",
            |_| async { Ok(()) },
            move |url| {
                browser_urls_for_open
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(url.to_owned());
                Ok(())
            },
        )
        .await;
        assert!(browser_urls.lock().unwrap().is_empty());

        open_ui_with_browser_fallback(
            ClientUiRole::Master,
            None,
            "http://127.0.0.1:8082/",
            |_| async { panic!("ineligible WebView must not launch") },
            |url| {
                browser_urls
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(url.to_owned());
                Ok(())
            },
        )
        .await;
        assert_eq!(*browser_urls.lock().unwrap(), ["http://127.0.0.1:8082/"]);
    }

    #[tokio::test]
    async fn webview_parent_pipe_stays_open_until_helper_exits() {
        struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropSignal {
            fn drop(&mut self) {
                if let Some(signal) = self.0.take() {
                    let _ = signal.send(());
                }
            }
        }

        let (helper_exit_tx, helper_exit_rx) = tokio::sync::oneshot::channel();
        let (pipe_dropped_tx, mut pipe_dropped_rx) = tokio::sync::oneshot::channel();
        retain_local_ui_lifetime_pipe(
            async move {
                let _ = helper_exit_rx.await;
                Ok(())
            },
            DropSignal(Some(pipe_dropped_tx)),
            "Worker",
        );

        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut pipe_dropped_rx)
                .await
                .is_err(),
            "parent pipe must remain open while helper is running"
        );
        helper_exit_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), pipe_dropped_rx)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn resolve_nodepool_endpoint_prefers_explicit_then_default() {
        let mut config = HivemindConfig::default();
        config.server.nodepool_grpc_endpoint = None;
        config.server.nodepool_grpc_addr = "0.0.0.0:50051".into();
        assert_eq!(
            resolve_nodepool_grpc_endpoint(&config),
            DEFAULT_NODEPOOL_GRPC_ENDPOINT
        );

        config.server.nodepool_grpc_endpoint = Some("custom-nodepool:50051".into());
        assert_eq!(
            resolve_nodepool_grpc_endpoint(&config),
            "custom-nodepool:50051"
        );
    }

    #[test]
    fn advertised_endpoint_parsing_ignores_secrets_and_rejects_invalid_targets() {
        let response: WebsiteVpnConfigResponse = serde_json::from_value(serde_json::json!({
            "success": true,
            "auth_key": "tskey-auth-secret",
            "config_text": "# wireguard_private_key=never-log-this\n# nodepool_grpc_endpoint=100.64.0.4:50051\n# another_secret=also-secret"
        }))
        .unwrap();
        assert_eq!(
            parse_advertised_nodepool_endpoint(&response.config_text).as_deref(),
            Some("100.64.0.4:50051")
        );
        for value in [
            "127.0.0.1:50051",
            "192.0.2.1:50051",
            "100.64.0.1:0",
            "100.64.0.1:invalid",
            "worker-a:50051",
            "100.64.0.1:50051/secret",
            "user@100.64.0.1:50051",
        ] {
            assert_eq!(
                parse_advertised_nodepool_endpoint(&format!("# nodepool_grpc_endpoint={value}")),
                None
            );
        }
        assert_eq!(
            parse_advertised_nodepool_endpoint(
                "# nodepool_grpc_endpoint=100.64.0.4:50051\n# nodepool_grpc_endpoint=100.64.0.5:50051"
            ),
            None
        );
    }

    #[test]
    fn stored_target_requires_matching_role_identity_and_no_operator_override() {
        let plan = test_reconnect_plan();
        let marker = serde_json::json!({
            "version": 2,
            "role": "worker",
            "login_server": "https://headscale.example",
            "hostname": "worker-test",
            "nodepool_target": "100.64.0.4:50051",
        });
        assert_eq!(
            persisted_target_from_marker(&marker, &plan).as_deref(),
            Some("100.64.0.4:50051")
        );
        let mut operator = plan.clone();
        operator.operator_endpoint = true;
        assert_eq!(persisted_target_from_marker(&marker, &operator), None);
        for (field, replacement) in [
            ("role", "master"),
            ("login_server", "https://other.example"),
            ("hostname", "other-worker"),
            ("nodepool_target", "127.0.0.1:50051"),
        ] {
            let mut invalid = marker.clone();
            invalid[field] = serde_json::json!(replacement);
            assert_eq!(persisted_target_from_marker(&invalid, &plan), None);
        }
        let mut legacy = marker;
        legacy["version"] = serde_json::json!(1);
        assert!(marker_matches_reconnect_plan(&legacy, &plan));
        assert_eq!(persisted_target_from_marker(&legacy, &plan), None);
    }

    #[test]
    fn extract_nodepool_peer_ips_matches_hostname_and_dns_name() {
        let status = serde_json::json!({
            "Peer": {
                "nodekey:abc": {
                    "HostName": "hivemind-nodepool",
                    "DNSName": "hivemind-nodepool.hivemind.local.",
                    "Online": true,
                    "TailscaleIPs": ["100.64.0.4", "fd7a:115c:a1e0::4"]
                },
                "nodekey:other": {
                    "HostName": "worker-a",
                    "Online": true,
                    "TailscaleIPs": ["100.64.0.20"]
                },
                "nodekey:offline": {
                    "HostName": "hivemind-nodepool",
                    "Online": false,
                    "TailscaleIPs": ["100.64.0.9"]
                },
                "nodekey:fake": {
                    "HostName": "hivemind-nodepool-impersonator",
                    "Online": true,
                    "TailscaleIPs": ["100.64.0.10"]
                }
            }
        });
        let ips = extract_nodepool_peer_ips(&status, &[DEFAULT_NODEPOOL_VPN_HOSTNAME.to_string()]);
        assert_eq!(ips, vec!["100.64.0.4".to_string()]);
    }

    #[test]
    fn worker_forward_port_uses_configured_listener_address() {
        assert_eq!(
            endpoint_port_for_worker(ClientRole::Worker, Some("0.0.0.0:60053")),
            60053
        );
        assert_eq!(
            endpoint_port_for_worker(ClientRole::Worker, Some("[::]:60054")),
            60054
        );
        assert_eq!(
            endpoint_port_for_worker(ClientRole::Master, Some("0.0.0.0:60055")),
            50053
        );
    }

    #[test]
    fn format_host_port_handles_ipv6() {
        assert_eq!(format_host_port("100.64.0.4", 50051), "100.64.0.4:50051");
        assert_eq!(format_host_port("fd7a::1", 50051), "[fd7a::1]:50051");
    }

    #[test]
    fn sanitize_hostname_accepts_website_client_ids() {
        assert_eq!(
            sanitize_hostname("user:localclient1:linux-join-your-nodepool"),
            "user-localclient1-linux-join-your-nodepool"
        );
        assert_eq!(
            sanitize_hostname("Master/Name With Spaces"),
            "master-name-with-spaces"
        );
        assert!(sanitize_hostname(":::")
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-'));
    }

    #[test]
    fn bounded_hostname_stays_within_headscale_label_limit() {
        let hostname = bounded_hostname(
            "user-e2e-d3a85169f0-hivemind-worker-a2137eb8728763c3d15ccd763222a58e",
        );
        assert!(hostname.len() <= 63);
        assert!(!hostname.starts_with('-'));
        assert!(!hostname.ends_with('-'));
        assert!(hostname
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn empty_hostname_gets_safe_fallback() {
        assert_eq!(bounded_hostname("---"), "hivemind-node");
    }

    #[test]
    fn endpoint_host_parses_ipv4_ipv6_and_schemes() {
        assert_eq!(
            endpoint_host("100.64.0.4:50051").as_deref(),
            Some("100.64.0.4")
        );
        assert_eq!(
            endpoint_host("http://100.64.0.4:50051").as_deref(),
            Some("100.64.0.4")
        );
        assert_eq!(endpoint_host("[fd7a::1]:50051").as_deref(), Some("fd7a::1"));
        assert_eq!(
            endpoint_host("https://[fd7a::1]:50051/").as_deref(),
            Some("fd7a::1")
        );
    }

    #[tokio::test]
    async fn nodepool_endpoint_probe_requires_http2_handshake() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener should bind");
        let address = listener
            .local_addr()
            .expect("loopback listener should report an address");

        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("probe should connect");
            tokio::time::sleep(Duration::from_millis(150)).await;
        });

        assert!(!nodepool_endpoint_reachable(&address.to_string()).await);
    }

    #[test]
    fn device_client_name_is_stable_and_role_scoped() {
        assert_eq!(
            client_name_for_device(ClientRole::Master, "0123456789abcdef"),
            "hivemind-master-0123456789abcdef"
        );
        assert_eq!(
            client_name_for_device(ClientRole::Worker, "0123456789abcdef"),
            "hivemind-worker-0123456789abcdef"
        );
    }

    #[test]
    fn matching_vpn_session_requires_the_same_non_secret_transport_spec() {
        let session = test_vpn_session();
        let plan = test_reconnect_plan();
        assert!(session_matches_reconnect_plan(&session, &plan));

        // Auth-key rotation alone must not tear down a healthy userspace tunnel.
        let mut rotated_key = plan.clone();
        rotated_key.auth_key = Some("rotated-test-key".into());
        assert!(session_matches_reconnect_plan(&session, &rotated_key));

        let mut different_target = plan.clone();
        different_target.configured_endpoint = "100.64.0.2:50051".into();
        assert!(!session_matches_reconnect_plan(&session, &different_target));

        let mut different_server = plan.clone();
        different_server.login_server = "https://other-headscale.example".into();
        assert!(!session_matches_reconnect_plan(&session, &different_server));

        let mut different_port = plan;
        different_port.worker_grpc_addr = Some("127.0.0.1:60053".into());
        assert!(!session_matches_reconnect_plan(&session, &different_port));
    }

    #[test]
    fn keepalive_is_claimed_once_per_role_until_its_task_exits() {
        let _runtime = runtime_lock();
        clear_runtime_for_test(ClientRole::Worker);
        assert!(claim_vpn_keepalive(ClientRole::Worker));
        assert!(!claim_vpn_keepalive(ClientRole::Worker));
        release_vpn_keepalive(ClientRole::Worker);
        assert!(claim_vpn_keepalive(ClientRole::Worker));
        clear_runtime_for_test(ClientRole::Worker);
    }

    #[test]
    fn vpn_recovery_is_debounced_until_three_consecutive_failures() {
        assert!(!should_reconnect_vpn(0));
        assert!(!should_reconnect_vpn(1));
        assert!(!should_reconnect_vpn(VPN_KEEPALIVE_FAILURE_THRESHOLD - 1));
        assert!(should_reconnect_vpn(VPN_KEEPALIVE_FAILURE_THRESHOLD));
    }

    #[tokio::test]
    async fn stale_keepalive_generation_cannot_retire_a_newer_session() {
        let _runtime = runtime_lock();
        clear_runtime_for_test(ClientRole::Worker);
        install_vpn_session(Arc::new(test_vpn_session()), test_reconnect_plan());
        let (_, generation, _) = vpn_runtime_snapshot(ClientRole::Worker);

        assert!(!retire_vpn_session_if_generation(
            ClientRole::Worker,
            generation.wrapping_add(1)
        ));
        assert!(current_vpn_session(ClientRole::Worker).await.is_some());
        assert!(retire_vpn_session_if_generation(
            ClientRole::Worker,
            generation
        ));
        assert!(current_vpn_session(ClientRole::Worker).await.is_none());
        clear_runtime_for_test(ClientRole::Worker);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn local_api_peer_discovery_requires_session_credential_and_online_nodepool() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0u8; 4096];
            let read = socket.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..read]).to_ascii_lowercase();
            assert!(request.starts_with("get /localapi/v0/status?peers=true http/1.1"));
            assert!(request.contains("sec-tailscale: localapi"));
            assert!(request.contains("authorization: basic onrlc3qtbg9jywwty3jlza=="));
            let body = serde_json::json!({"Peer": {
                "nodepool": {"HostName":"hivemind-nodepool", "Online":true, "TailscaleIPs":["100.64.0.4"]},
                "offline": {"HostName":"hivemind-nodepool", "Online":false, "TailscaleIPs":["100.64.0.9"]},
                "worker": {"HostName":"worker-a", "Online":true, "TailscaleIPs":["100.64.0.8"]}
            }}).to_string();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let status = local_api_status(&addr.to_string(), "test-local-cred")
            .await
            .unwrap();
        let ips = extract_nodepool_peer_ips(&status, &[DEFAULT_NODEPOOL_VPN_HOSTNAME.to_string()]);
        assert_eq!(ips, vec!["100.64.0.4"]);
        server.await.unwrap();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn local_api_denial_does_not_expose_credential_or_response_body() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0u8; 4096];
            let _ = socket.read(&mut bytes).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 14\r\n\r\nsecret-response",
                )
                .await
                .unwrap();
        });
        let error = local_api_status(&addr.to_string(), "secret-password")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("401"));
        assert!(!error.contains("secret-password"));
        assert!(!error.contains("secret-response"));
        server.await.unwrap();
    }

    #[cfg(target_os = "windows")]
    #[derive(Clone)]
    struct ProbeGrpcService;

    #[cfg(target_os = "windows")]
    impl tonic::server::NamedService for ProbeGrpcService {
        const NAME: &'static str = "hivemind.client_runtime.TransportProbe";
    }

    #[cfg(target_os = "windows")]
    impl tonic::codegen::Service<tonic::codegen::http::Request<tonic::body::Body>>
        for ProbeGrpcService
    {
        type Response = tonic::codegen::http::Response<tonic::body::Body>;
        type Error = std::convert::Infallible;
        type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(
            &mut self,
            _request: tonic::codegen::http::Request<tonic::body::Body>,
        ) -> Self::Future {
            let response = tonic::codegen::http::Response::builder()
                .header("content-type", "application/grpc")
                .header("grpc-status", "12")
                .body(tonic::body::Body::empty())
                .unwrap();
            std::future::ready(Ok(response))
        }
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn socks_candidate_probe_promotes_only_verified_bridge() {
        let grpc = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let grpc_addr = grpc.local_addr().unwrap();
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(grpc);
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ProbeGrpcService)
                .serve_with_incoming(incoming)
                .await
                .unwrap();
        });
        let socks = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socks_addr = socks.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let (mut socket, _) = socks.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [5, 1, 2]);
            socket.write_all(&[5, 2]).await.unwrap();
            let mut username_len = [0u8; 2];
            socket.read_exact(&mut username_len).await.unwrap();
            assert_eq!(username_len, [1, 5]);
            let mut username = [0u8; 5];
            socket.read_exact(&mut username).await.unwrap();
            assert_eq!(&username, b"tsnet");
            let mut password_len = [0u8; 1];
            socket.read_exact(&mut password_len).await.unwrap();
            let mut password = vec![0u8; usize::from(password_len[0])];
            socket.read_exact(&mut password).await.unwrap();
            assert_eq!(password, b"test-credential");
            socket.write_all(&[1, 0]).await.unwrap();
            let mut request = [0u8; 10];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(&request[..8], &[5, 1, 0, 1, 100, 64, 0, 4]);
            socket
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                .await
                .unwrap();
            let mut upstream = TcpStream::connect(grpc_addr).await.unwrap();
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
        });
        let mut session = test_vpn_session();
        session.bridge_addr = None;
        session.userspace_socks_addr = Some(socks_addr.to_string());
        session.userspace_proxy_cred = Some("test-credential".into());
        let endpoint = probe_nodepool_target(&session, "100.64.0.4:50051", Duration::from_secs(3))
            .await
            .unwrap()
            .expect("gRPC over SOCKS should pass the transport probe");
        assert_eq!(
            session.bridge_endpoint().as_deref(),
            Some(endpoint.as_str())
        );
        assert_eq!(
            session.active_nodepool_target().as_deref(),
            Some("100.64.0.4:50051")
        );
        session.shutdown();
        server.abort();
        proxy.abort();
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn failed_candidate_bridge_is_not_active() {
        let mut session = test_vpn_session();
        session.bridge_addr = None;
        session.userspace_socks_addr = Some("127.0.0.1:9".into());
        session.userspace_proxy_cred = Some("test-credential".into());
        let result = probe_nodepool_target(&session, "100.64.0.4:50051", Duration::from_secs(3))
            .await
            .unwrap();
        assert_eq!(result, None);
        assert_eq!(session.bridge_endpoint(), None);
        assert_eq!(session.active_nodepool_target(), None);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn explicit_failed_target_does_not_probe_peers_or_wait_full_startup_window() {
        let mut session = test_vpn_session();
        session.bridge_addr = None;
        session.userspace_socks_addr = Some("127.0.0.1:9".into());
        session.userspace_proxy_cred = Some("test-credential".into());
        let mut plan = test_reconnect_plan();
        plan.operator_endpoint = true;
        plan.configured_endpoint = "100.64.0.4:50051".into();
        let start = Instant::now();
        let error = wait_for_nodepool_after_join(&session, &plan, Duration::from_secs(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("no automatic reroute"));
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(session.bridge_endpoint(), None);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn cancelled_candidate_probe_closes_unpublished_bridge() {
        let socks = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socks_addr = socks.local_addr().unwrap();
        let (request_started, request_started_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(2), socks.accept())
                .await
                .unwrap()
                .unwrap();
            let mut greeting = [0u8; 3];
            if tokio::time::timeout(Duration::from_secs(2), socket.read_exact(&mut greeting))
                .await
                .unwrap()
                .is_err()
            {
                return 0;
            }
            let _ = request_started.send(());
            let mut byte = [0u8; 1];
            tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                .await
                .unwrap()
                .unwrap()
        });
        let mut session = test_vpn_session();
        session.bridge_addr = None;
        session.userspace_socks_addr = Some(socks_addr.to_string());
        session.userspace_proxy_cred = Some("test-credential".into());
        let mut probe = Box::pin(probe_nodepool_target(
            &session,
            "100.64.0.4:50051",
            Duration::from_secs(3),
        ));
        tokio::select! {
            result = probe.as_mut() => panic!("candidate probe ended before SOCKS handshake stalled: {result:?}"),
            started = tokio::time::timeout(Duration::from_secs(2), request_started_rx) => {
                started.unwrap().unwrap();
            }
        }
        let cancelled = tokio::time::timeout(Duration::from_millis(100), probe.as_mut()).await;
        assert!(cancelled.is_err());
        drop(probe);
        assert_eq!(session.bridge_endpoint(), None);
        assert_eq!(server.await.unwrap(), 0);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn active_bridge_does_not_revert_to_initial_target() {
        let mut session = test_vpn_session();
        session.bridge_addr = None;
        let active = start_socks_bridge("127.0.0.1:9", "test-credential", "100.64.0.4:50051")
            .await
            .unwrap();
        session.activate_bridge("100.64.0.4:50051".into(), active.clone());
        assert_eq!(session.bridge_endpoint(), Some(active.addr().to_string()));
        assert_eq!(
            session.active_nodepool_target().as_deref(),
            Some("100.64.0.4:50051")
        );
        session.shutdown();
        assert_eq!(session.bridge_endpoint(), None);
    }

    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn closing_socks_bridge_releases_its_listener() {
        let bridge = start_socks_bridge("127.0.0.1:9", "test-credential", "100.64.0.1:50051")
            .await
            .expect("bridge should bind a local listener");
        let address = bridge.addr();
        bridge.close();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let connection =
            tokio::time::timeout(Duration::from_millis(150), TcpStream::connect(address)).await;
        assert!(!matches!(connection, Ok(Ok(_))));
    }

    #[test]
    fn vpn_status_never_contains_auth_key() {
        let status = VpnBootstrapStatus::ready("127.0.0.1:1234", Some("100.64.0.9"));
        assert_eq!(status.state, VpnBootstrapState::Ready);
        assert!(!format!("{status:?}").contains("tskey-auth"));
    }
}
