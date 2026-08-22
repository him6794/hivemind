//! Shared runtime helpers for downloaded master/worker clients.
//!
//! Product model (AGENTS.md): a user-deployed master or worker should:
//! 1. start its local HTTP + bundled UI
//! 2. obtain VPN bootstrap config from the official website-api on login
//! 3. join the configured overlay automatically (embedded libtailscale on Windows)
//! 4. reach the platform nodepool over the overlay
//!
//! Users must not hand-copy pre-auth keys after install.

use anyhow::{bail, Context, Result};
use hivemind_config::HivemindConfig;
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
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
#[cfg(target_os = "windows")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(target_os = "windows")]
use tokio::net::TcpListener;
use tokio::net::TcpStream;
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
    fn as_str(self) -> &'static str {
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

#[derive(Debug, Clone, Deserialize)]
struct WebsiteVpnConfigResponse {
    success: bool,
    #[serde(default)]
    login_server: String,
    #[serde(default)]
    auth_key: String,
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

/// VPN session state
pub struct VpnSession {
    pub role: ClientRole,
    pub transport: VpnTransport,
    pub state_dir: PathBuf,
    pub bridge_addr: Option<SocketAddr>,
    pub overlay_ip: Option<String>,
    #[cfg(target_os = "windows")]
    pub userspace_socks_addr: Option<String>,
    #[cfg(target_os = "windows")]
    pub userspace_proxy_cred: Option<String>,
    pub auth_key: String,
    pub login_server: String,
    pub hostname: String,
    // WireGuard specific fields
    pub wg_private_key: Option<boringtun::x25519::StaticSecret>,
    pub wg_peer_public_key: Option<boringtun::x25519::PublicKey>,
    pub wg_endpoint: Option<SocketAddr>,
    pub wg_allowed_ips: Option<String>,
    pub wg_tunnel: Option<Arc<TokioMutex<wireguard::WireguardTunnel>>>,
    #[cfg(target_os = "windows")]
    pub libtailscale: Option<Arc<LibtailscaleSession>>,
}

#[cfg(target_os = "windows")]
pub struct LibtailscaleSession {
    handle: i32,
}

#[cfg(target_os = "windows")]
unsafe impl Send for LibtailscaleSession {}
#[cfg(target_os = "windows")]
unsafe impl Sync for LibtailscaleSession {}

#[cfg(target_os = "windows")]
impl Drop for LibtailscaleSession {
    fn drop(&mut self) {
        unsafe {
            tailscale_close(self.handle);
        }
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
            _module: *mut c_void,
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

        unsafe impl Send for Api {}
        unsafe impl Sync for Api {}

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
                    _module: module,
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
    /// Get the bridge endpoint for gRPC forwarding
    pub fn bridge_endpoint(&self) -> Option<String> {
        self.bridge_addr.map(|addr| addr.to_string())
    }
}

/// Global VPN session storage
static VPN_SESSIONS: OnceLock<StdMutex<HashMap<ClientRole, Arc<VpnSession>>>> = OnceLock::new();
static VPN_STATUSES: OnceLock<StdMutex<HashMap<ClientRole, VpnBootstrapStatus>>> = OnceLock::new();
static VPN_BOOTSTRAP_LOCKS: OnceLock<StdMutex<HashMap<ClientRole, Arc<TokioMutex<()>>>>> =
    OnceLock::new();

fn sessions_map() -> &'static StdMutex<HashMap<ClientRole, Arc<VpnSession>>> {
    VPN_SESSIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn statuses_map() -> &'static StdMutex<HashMap<ClientRole, VpnBootstrapStatus>> {
    VPN_STATUSES.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn bootstrap_locks_map() -> &'static StdMutex<HashMap<ClientRole, Arc<TokioMutex<()>>>> {
    VPN_BOOTSTRAP_LOCKS.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn bootstrap_lock(role: ClientRole) -> Arc<TokioMutex<()>> {
    let mut locks = bootstrap_locks_map().lock().unwrap();
    locks
        .entry(role)
        .or_insert_with(|| Arc::new(TokioMutex::new(())))
        .clone()
}

fn set_vpn_status(role: ClientRole, status: VpnBootstrapStatus) {
    statuses_map().lock().unwrap().insert(role, status);
}

/// Return the non-secret current bootstrap state for a client role.
pub fn current_vpn_status(role: ClientRole) -> VpnBootstrapStatus {
    statuses_map()
        .lock()
        .unwrap()
        .get(&role)
        .cloned()
        .unwrap_or_else(|| VpnBootstrapStatus::new(VpnBootstrapState::AwaitingLogin, None))
}

/// Store a VPN session
#[allow(dead_code)]
async fn store_vpn_session(session: VpnSession) -> Arc<VpnSession> {
    let arc = Arc::new(session);
    sessions_map().lock().unwrap().insert(arc.role, arc.clone());
    arc
}

/// Get the current VPN session for a role
pub async fn current_vpn_session(role: ClientRole) -> Option<Arc<VpnSession>> {
    sessions_map().lock().unwrap().get(&role).cloned()
}

/// Clear the VPN session for a role
pub async fn clear_vpn_session(role: ClientRole) {
    sessions_map().lock().unwrap().remove(&role);
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
    let prefix = role.env_prefix();
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

    match plan_vpn_bootstrap(
        auth_key.as_deref(),
        login_server.as_deref(),
        hostname.as_deref(),
        Some(config.vpn.headscale_url.as_str()),
        role,
    )? {
        VpnBootstrapPlan::Skip => {
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
            let endpoint = if has_persisted_vpn_state(role) {
                match join_and_confirm_nodepool(
                    role,
                    None,
                    &login_server,
                    &hostname,
                    configured_endpoint,
                    worker_grpc_addr,
                    Duration::from_secs(config.vpn.startup_timeout_secs),
                )
                .await
                {
                    Ok(endpoint) => endpoint,
                    Err(err) => {
                        clear_vpn_session(role).await;
                        tracing::warn!(
                            "{} persisted VPN state could not rehydrate for explicit auth-key startup; resetting local state: {}",
                            role.as_str(),
                            err
                        );
                        reset_libtailscale_state_for_new_auth_key(role)?;
                        join_and_confirm_nodepool(
                            role,
                            Some(&auth_key),
                            &login_server,
                            &hostname,
                            configured_endpoint,
                            worker_grpc_addr,
                            Duration::from_secs(config.vpn.startup_timeout_secs),
                        )
                        .await?
                    }
                }
            } else {
                reset_libtailscale_state_for_new_auth_key(role)?;
                join_and_confirm_nodepool(
                    role,
                    Some(&auth_key),
                    &login_server,
                    &hostname,
                    configured_endpoint,
                    worker_grpc_addr,
                    Duration::from_secs(config.vpn.startup_timeout_secs),
                )
                .await?
            };
            set_ready_vpn_status(role, &endpoint).await;
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
    if env_auth_key_present(role) {
        let endpoint =
            ensure_env_vpn_for_endpoint(config, role, &resolve_nodepool_grpc_endpoint(config))
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("explicit VPN auth key disappeared during bootstrap")
                })?;
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
        let endpoint =
            ensure_env_vpn_for_endpoint(config, role, &resolve_nodepool_grpc_endpoint(config))
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("explicit VPN auth key disappeared during bootstrap")
                })?;
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
    if let Some(endpoint) = first_reachable_nodepool_endpoint(role, &configured_endpoint).await {
        set_ready_vpn_status(role, &endpoint).await;
        return Ok(Some(endpoint));
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
    let hostname = first_nonempty(&[
        env_trim(&format!("{}_VPN_HOSTNAME", role.env_prefix())),
        env_trim("HOSTNAME"),
        env_trim("COMPUTERNAME"),
        Some(device_name.clone()),
    ])
    .unwrap_or_else(|| device_name.clone());

    set_vpn_status(
        role,
        VpnBootstrapStatus::new(VpnBootstrapState::Joining, None),
    );

    // A successful libtailscale state can reconnect without issuing another
    // one-time key. If that state is stale or revoked, fall through to the
    // authenticated Website API issuance path below.
    if has_persisted_vpn_state(role) {
        match join_and_confirm_nodepool(
            role,
            None,
            &login_server,
            &hostname,
            &configured_endpoint,
            worker_grpc_addr_for_role(config, role),
            Duration::from_secs(config.vpn.startup_timeout_secs),
        )
        .await
        {
            Ok(endpoint) => {
                set_ready_vpn_status(role, &endpoint).await;
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

    // A newly issued key must not be ignored by tsnet because a previous
    // failed/revoked session left a NeedsLogin state file behind.
    reset_libtailscale_state_for_new_auth_key(role)?;

    match join_and_confirm_nodepool(
        role,
        Some(vpn.auth_key.trim()),
        login_server.trim_end_matches('/'),
        &join_hostname,
        &configured_endpoint,
        worker_grpc_addr_for_role(config, role),
        Duration::from_secs(config.vpn.startup_timeout_secs),
    )
    .await
    {
        Ok(endpoint) => {
            set_ready_vpn_status(role, &endpoint).await;
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

async fn set_ready_vpn_status(role: ClientRole, endpoint: &str) {
    let overlay_ip = current_vpn_session(role)
        .await
        .and_then(|session| session.overlay_ip.clone());
    set_vpn_status(
        role,
        VpnBootstrapStatus {
            state: VpnBootstrapState::Ready,
            endpoint: Some(endpoint.to_string()),
            overlay_ip,
            message: None,
        },
    );
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
    if let Some(endpoint) = first_reachable_nodepool_endpoint(role, configured_endpoint).await {
        return Ok(endpoint);
    }

    let session = current_vpn_session(role).await;
    let candidates =
        nodepool_endpoint_candidates(role, configured_endpoint, session.as_deref()).await;
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
    let session = current_vpn_session(role).await;
    let candidates =
        nodepool_endpoint_candidates(role, configured_endpoint, session.as_deref()).await;
    // Probe candidates concurrently. Sequential 3-second probes made every
    // login wait for dead overlay/DNS candidates before trying the live one.
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
        if let Some(bridge) = session.bridge_endpoint() {
            push_unique(bridge);
            return candidates;
        }
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
    role: ClientRole,
    auth_key: Option<&str>,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
    startup_timeout: Duration,
) -> Result<Arc<VpnSession>> {
    tokio::time::timeout(
        startup_timeout.max(Duration::from_secs(1)),
        bring_up_vpn(
            role,
            auth_key,
            login_server,
            hostname,
            configured_endpoint,
            worker_grpc_addr,
        ),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "{} VPN startup exceeded the configured timeout of {:?}",
            role.as_str(),
            startup_timeout
        )
    })?
}

async fn join_and_confirm_nodepool(
    role: ClientRole,
    auth_key: Option<&str>,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
    startup_timeout: Duration,
) -> Result<String> {
    let startup_timeout = startup_timeout.max(Duration::from_secs(1));
    let hostname = bounded_hostname(hostname);
    let startup_deadline = Instant::now() + startup_timeout;
    let session = bring_up_vpn_bounded(
        role,
        auth_key,
        login_server,
        &hostname,
        configured_endpoint,
        worker_grpc_addr,
        startup_timeout,
    )
    .await?;
    let remaining = startup_deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!(
            "{} VPN startup exceeded the configured timeout of {:?} before Nodepool readiness",
            role.as_str(),
            startup_timeout
        );
    }
    let endpoint =
        wait_for_nodepool_after_join(role, session.as_ref(), configured_endpoint, remaining)
            .await?;
    if let Err(err) = mark_persisted_vpn_state(role, login_server, &hostname) {
        tracing::warn!(
            "{} VPN joined but its local state marker could not be persisted: {}",
            role.as_str(),
            err
        );
    }
    spawn_vpn_keepalive(
        role,
        auth_key,
        login_server,
        &hostname,
        configured_endpoint,
        worker_grpc_addr,
        startup_timeout,
    );
    Ok(endpoint)
}

fn spawn_vpn_keepalive(
    role: ClientRole,
    auth_key: Option<&str>,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
    startup_timeout: Duration,
) {
    let auth_key = auth_key.map(str::to_string);
    let login_server = login_server.to_string();
    let hostname = hostname.to_string();
    let configured_endpoint = configured_endpoint.to_string();
    let worker_grpc_addr = worker_grpc_addr.map(str::to_string);
    tokio::spawn(async move {
        vpn_keepalive_loop(
            role,
            auth_key,
            login_server,
            hostname,
            configured_endpoint,
            worker_grpc_addr,
            startup_timeout,
        )
        .await;
    });
}

async fn vpn_keepalive_loop(
    role: ClientRole,
    auth_key: Option<String>,
    login_server: String,
    hostname: String,
    configured_endpoint: String,
    worker_grpc_addr: Option<String>,
    startup_timeout: Duration,
) {
    let mut failures = 0u32;
    loop {
        sleep(Duration::from_secs(5)).await;
        let session = match current_vpn_session(role).await {
            Some(session) => session,
            None => {
                tracing::warn!(
                    "{} VPN keepalive: session missing; re-joining",
                    role.as_str()
                );
                match bring_up_vpn_bounded(
                    role,
                    auth_key.as_deref(),
                    &login_server,
                    &hostname,
                    &configured_endpoint,
                    worker_grpc_addr.as_deref(),
                    startup_timeout,
                )
                .await
                {
                    Ok(session) => {
                        match wait_for_nodepool_after_join(
                            role,
                            session.as_ref(),
                            &configured_endpoint,
                            startup_timeout,
                        )
                        .await
                        {
                            Ok(endpoint) => set_ready_vpn_status(role, &endpoint).await,
                            Err(err) => {
                                set_vpn_status(
                                    role,
                                    VpnBootstrapStatus::new(
                                        VpnBootstrapState::RetryableFailure,
                                        Some(err.to_string()),
                                    ),
                                );
                                tracing::warn!(
                                    "{} VPN rejoin nodepool readiness failed: {err}",
                                    role.as_str()
                                );
                            }
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
                        tracing::warn!("{} VPN rejoin failed: {err}", role.as_str());
                    }
                }
                continue;
            }
        };

        let ping_ok = wireguard_is_up(session.as_ref()).await.unwrap_or(false);
        let endpoint_ok = first_reachable_nodepool_endpoint(role, &configured_endpoint)
            .await
            .is_some();

        // A live tunnel alone is not sufficient; Nodepool must complete the
        // same gRPC transport probe used during startup.
        if endpoint_ok {
            if failures > 0 {
                tracing::info!(
                    "{} VPN keepalive restored (ping_ok={ping_ok}, endpoint_ok={endpoint_ok})",
                    role.as_str()
                );
            }
            failures = 0;
            continue;
        }

        failures = failures.saturating_add(1);
        set_vpn_status(
            role,
            VpnBootstrapStatus::new(
                VpnBootstrapState::RetryableFailure,
                Some("Nodepool readiness lost; reconnecting VPN".into()),
            ),
        );
        tracing::warn!(
            "{} VPN keepalive missed nodepool (streak={failures}); forcing reconnect",
            role.as_str()
        );
        match bring_up_vpn_bounded(
            role,
            auth_key.as_deref(),
            &login_server,
            &hostname,
            &configured_endpoint,
            worker_grpc_addr.as_deref(),
            startup_timeout,
        )
        .await
        {
            Ok(new_session) => {
                match wait_for_nodepool_after_join(
                    role,
                    new_session.as_ref(),
                    &configured_endpoint,
                    startup_timeout,
                )
                .await
                {
                    Ok(endpoint) => set_ready_vpn_status(role, &endpoint).await,
                    Err(err) => {
                        set_vpn_status(
                            role,
                            VpnBootstrapStatus::new(
                                VpnBootstrapState::RetryableFailure,
                                Some(err.to_string()),
                            ),
                        );
                        tracing::warn!(
                            "{} VPN reconnect nodepool readiness failed: {err}",
                            role.as_str()
                        );
                    }
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
            }
        }
    }
}

async fn wait_for_nodepool_after_join(
    role: ClientRole,
    session: &VpnSession,
    configured_endpoint: &str,
    startup_timeout: Duration,
) -> Result<String> {
    let timeout = startup_timeout;
    let deadline = Instant::now() + timeout;
    let mut last_err = None;
    let mut attempt = 0u32;

    loop {
        if Instant::now() >= deadline {
            break;
        }
        attempt = attempt.saturating_add(1);
        if attempt == 1 {
            tracing::info!(
                "{} probing nodepool gRPC reachability after VPN join (configured endpoint: {}, timeout: {:?})",
                role.as_str(),
                configured_endpoint,
                timeout
            );
        }
        // Check WireGuard tunnel is up periodically without making it the only
        // readiness signal; the protocol probe below is authoritative.
        if attempt == 1 || attempt.is_multiple_of(4) {
            let _ = wireguard_is_up(session).await;
        }
        match first_reachable_nodepool_endpoint(role, configured_endpoint).await {
            Some(endpoint) => {
                // Ensure bridge (if any) is pointed at the live peer IP.
                if let Some(ip) = endpoint_host(&endpoint) {
                    if ip != "127.0.0.1" && !ip.starts_with("127.") {
                        ensure_userspace_bridge(session, &ip).await.ok();
                        if let Some(bridge) = current_vpn_session(role)
                            .await
                            .as_ref()
                            .and_then(|s| s.bridge_endpoint())
                        {
                            if nodepool_endpoint_reachable(&bridge).await {
                                tracing::info!(
                                    "{} nodepool reachable via userspace bridge {} (peer {})",
                                    role.as_str(),
                                    bridge,
                                    endpoint
                                );
                                return Ok(bridge);
                            }
                        }
                    }
                }
                if attempt > 1 {
                    tracing::info!(
                        "{} nodepool endpoint {} became reachable after {} probe(s)",
                        role.as_str(),
                        endpoint,
                        attempt
                    );
                }
                tracing::info!(
                    "{} nodepool gRPC connectivity probe succeeded: {}",
                    role.as_str(),
                    endpoint
                );
                return Ok(endpoint);
            }
            None => {
                last_err = Some(anyhow::anyhow!(
                    "no candidate completed the gRPC transport handshake"
                ));
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                sleep(remaining.min(Duration::from_millis(500))).await;
            }
        }
    }

    let current_session = current_vpn_session(role).await;
    let candidates = nodepool_endpoint_candidates(
        role,
        configured_endpoint,
        current_session.as_deref().or(Some(session)),
    )
    .await;
    bail!(
        "nodepool endpoint is still unreachable after VPN bootstrap (tried: {}). Check that the Headscale session is online and that the platform nodepool VPN sidecar ({}) is online{}",
        if candidates.is_empty() {
            configured_endpoint.to_string()
        } else {
            candidates.join(", ")
        },
        DEFAULT_NODEPOOL_VPN_HOSTNAME,
        last_err
            .map(|e| format!(": {e}"))
            .unwrap_or_default()
    )
}

/// Start the bundled Tailscale userspace VPN and expose its overlay through a
/// localhost SOCKS bridge. This is required on Windows, where userspace mode
/// does not install a kernel route for ordinary gRPC sockets.
async fn bring_up_vpn(
    role: ClientRole,
    auth_key: Option<&str>,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
) -> Result<Arc<VpnSession>> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (
            role,
            auth_key,
            login_server,
            hostname,
            configured_endpoint,
            worker_grpc_addr,
        );
        bail!("embedded libtailscale is currently only packaged for Windows");
    }

    #[cfg(target_os = "windows")]
    {
        bring_up_vpn_windows(
            role,
            auth_key,
            login_server,
            hostname,
            configured_endpoint,
            worker_grpc_addr,
        )
        .await
    }
}

#[cfg(target_os = "windows")]
async fn bring_up_vpn_windows(
    role: ClientRole,
    auth_key: Option<&str>,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
    worker_grpc_addr: Option<&str>,
) -> Result<Arc<VpnSession>> {
    ensure_libtailscale_loaded().map_err(|error| anyhow::anyhow!(error))?;
    let hostname = sanitize_hostname(hostname);
    let state_dir = vpn_state_dir(role);
    std::fs::create_dir_all(&state_dir).with_context(|| {
        format!(
            "failed to create {} VPN state dir {}",
            role.as_str(),
            state_dir.display()
        )
    })?;

    #[cfg(target_os = "windows")]
    let (vpn_handle, loopback_addr, proxy_cred, overlay_ip) =
        start_libtailscale(&state_dir, &hostname, auth_key, login_server).await?;
    #[cfg(target_os = "windows")]
    let network = CString::new("tcp")?;
    #[cfg(target_os = "windows")]
    let tailnet_addr = CString::new(format!(
        ":{}",
        endpoint_port_for_worker(role, worker_grpc_addr)
    ))?;
    #[cfg(target_os = "windows")]
    let local_addr = CString::new(format!(
        "127.0.0.1:{}",
        endpoint_port_for_worker(role, worker_grpc_addr)
    ))?;
    #[cfg(target_os = "windows")]
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
        configured_endpoint
    );
    let bridge_target = normalize_nodepool_endpoint(configured_endpoint);
    let bridge_addr = start_socks_bridge(&loopback_addr, &proxy_cred, &bridge_target).await?;

    let session = VpnSession {
        role,
        transport: VpnTransport::Tailscale,
        state_dir,
        bridge_addr: Some(bridge_addr),
        overlay_ip,
        #[cfg(target_os = "windows")]
        userspace_socks_addr: Some(loopback_addr),
        #[cfg(target_os = "windows")]
        userspace_proxy_cred: Some(proxy_cred),
        // A freshly issued key is retained only in process memory for the
        // keepalive path. Rehydrated sessions intentionally carry an empty
        // value and rely on the next authenticated bootstrap if rejoin fails.
        auth_key: auth_key.unwrap_or_default().to_string(),
        login_server: login_server.to_string(),
        hostname: hostname.to_string(),
        wg_private_key: None,
        wg_peer_public_key: None,
        wg_endpoint: None,
        wg_allowed_ips: None,
        wg_tunnel: None,
        #[cfg(target_os = "windows")]
        libtailscale: Some(vpn_handle),
    };
    Ok(store_vpn_session(session).await)
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
        return Ok(start_socks_bridge(socks, cred, &bridge_target)
            .await?
            .to_string());
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
) -> Result<(Arc<LibtailscaleSession>, String, String, Option<String>)> {
    let state_dir = state_dir.to_path_buf();
    let hostname = CString::new(hostname)?;
    let auth_key = auth_key.map(CString::new).transpose()?;
    let login_server = CString::new(login_server)?;
    tokio::task::spawn_blocking(move || unsafe {
        let handle = tailscale_new();
        if handle < 0 {
            bail!("libtailscale failed to allocate a session");
        }
        let fail = |message: &str| -> anyhow::Error {
            let mut buf = vec![0i8; 2048];
            let detail = if tailscale_errmsg(handle, buf.as_mut_ptr(), buf.len()) == 0 {
                CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
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
            (tailscale_set_dir(handle, dir.as_ptr()), "set state dir"),
            (
                tailscale_set_hostname(handle, hostname.as_ptr()),
                "set hostname",
            ),
        ] {
            if ok != 0 {
                let err = fail(name);
                tailscale_close(handle);
                return Err(err);
            }
        }
        if let Some(auth_key) = auth_key.as_ref() {
            if tailscale_set_authkey(handle, auth_key.as_ptr()) != 0 {
                let err = fail("set auth key");
                tailscale_close(handle);
                return Err(err);
            }
        }
        if tailscale_set_control_url(handle, login_server.as_ptr()) != 0 {
            let err = fail("set control URL");
            tailscale_close(handle);
            return Err(err);
        }
        tracing::info!(
            "embedded libtailscale starting Headscale {}",
            login_server.to_string_lossy()
        );
        if tailscale_up(handle) != 0 {
            let err = fail("libtailscale Headscale join failed");
            tailscale_close(handle);
            return Err(err);
        }
        let mut addr = vec![0i8; 128];
        let mut proxy = vec![0i8; 64];
        let mut local_api = vec![0i8; 64];
        if tailscale_loopback(
            handle,
            addr.as_mut_ptr(),
            addr.len(),
            proxy.as_mut_ptr(),
            local_api.as_mut_ptr(),
        ) != 0
        {
            let err = fail("libtailscale loopback SOCKS failed");
            tailscale_close(handle);
            return Err(err);
        }
        let addr = CStr::from_ptr(addr.as_ptr()).to_string_lossy().into_owned();
        let proxy = CStr::from_ptr(proxy.as_ptr())
            .to_string_lossy()
            .into_owned();
        let mut ips = vec![0i8; 128];
        let overlay_ip = if tailscale_getips(handle, ips.as_mut_ptr(), ips.len()) == 0 {
            CStr::from_ptr(ips.as_ptr())
                .to_string_lossy()
                .split(',')
                .find(|ip| !ip.is_empty() && !ip.contains(':'))
                .map(str::to_string)
        } else {
            None
        };
        Ok((
            Arc::new(LibtailscaleSession { handle }),
            addr,
            proxy,
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
) -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let local = listener.local_addr()?;
    let socks_addr = socks_addr.to_string();
    let proxy_cred = proxy_cred.to_string();
    let target = target.to_string();
    tokio::spawn(async move {
        loop {
            let Ok((client, _)) = listener.accept().await else {
                break;
            };
            let socks_addr = socks_addr.clone();
            let proxy_cred = proxy_cred.clone();
            let target = target.clone();
            tokio::spawn(async move {
                if let Err(err) = proxy_socks5(client, &socks_addr, &proxy_cred, &target).await {
                    tracing::debug!("Tailscale SOCKS bridge connection failed: {err}");
                }
            });
        }
    });
    Ok(local)
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

/// Check if WireGuard tunnel is up
async fn wireguard_is_up(session: &VpnSession) -> Result<bool> {
    if session.transport != VpnTransport::Wireguard {
        return Ok(false);
    }

    if let Some(wg_tunnel) = &session.wg_tunnel {
        let tunnel = wg_tunnel.lock().await;
        Ok(tunnel.is_connected().await)
    } else {
        Ok(false)
    }
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

/// Ensure a userspace TCP bridge for gRPC over WireGuard
async fn ensure_userspace_bridge(session: &VpnSession, _peer_ip: &str) -> Result<()> {
    if session.bridge_addr.is_some() {
        // Bridge already running on the WireGuard local address
        // The WireGuard tunnel already provides the network path
        return Ok(());
    }
    Ok(())
}

/// Get the VPN state directory for a role.
fn vpn_state_dir(role: ClientRole) -> PathBuf {
    let base = dirs::data_dir()
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

fn has_persisted_vpn_state(role: ClientRole) -> bool {
    state_marker_path(role).is_file()
}

fn mark_persisted_vpn_state(role: ClientRole, login_server: &str, hostname: &str) -> Result<()> {
    let state_dir = vpn_state_dir(role);
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("failed to create VPN state dir {}", state_dir.display()))?;
    let marker = serde_json::json!({
        "version": 1,
        "role": role.as_str(),
        "login_server": login_server,
        "hostname": hostname,
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

/// Extract nodepool peer IPs from Tailscale status JSON.
/// Returns IPv4 addresses for peers matching the given hostnames.
#[allow(dead_code)]
fn extract_nodepool_peer_ips(status: &serde_json::Value, hostnames: &[String]) -> Vec<String> {
    let mut ips = Vec::new();
    if let Some(peer_map) = status.get("Peer").and_then(|v| v.as_object()) {
        for (_, peer_info) in peer_map {
            let hostname_match = peer_info
                .get("HostName")
                .and_then(|v| v.as_str())
                .map(|hn| hostnames.iter().any(|h| h == hn))
                .unwrap_or(false);
            let dns_name_match = peer_info
                .get("DNSName")
                .and_then(|v| v.as_str())
                .map(|dn| {
                    hostnames
                        .iter()
                        .any(|h| dn.starts_with(&format!("{h}.")) || dn == h)
                })
                .unwrap_or(false);

            if hostname_match || dns_name_match {
                if let Some(tailscale_ips) =
                    peer_info.get("TailscaleIPs").and_then(|v| v.as_array())
                {
                    for ip_val in tailscale_ips {
                        if let Some(ip_str) = ip_val.as_str() {
                            // Only return IPv4 addresses
                            if ip_str.parse::<std::net::Ipv4Addr>().is_ok() {
                                ips.push(ip_str.to_string());
                            }
                        }
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
    fn local_ui_url_rewrites_unspecified_bind_addresses() {
        assert_eq!(local_ui_url("0.0.0.0:8082"), "http://127.0.0.1:8082/");
        assert_eq!(local_ui_url("127.0.0.1:18080"), "http://127.0.0.1:18080/");
        assert_eq!(local_ui_url("[::]:8082"), "http://127.0.0.1:8082/");
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
    fn vpn_status_never_contains_auth_key() {
        let status = VpnBootstrapStatus::ready("127.0.0.1:1234", Some("100.64.0.9"));
        assert_eq!(status.state, VpnBootstrapState::Ready);
        assert!(!format!("{status:?}").contains("tskey-auth"));
    }
}
