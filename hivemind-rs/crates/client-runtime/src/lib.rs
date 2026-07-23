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
use serde::Deserialize;
use std::collections::HashMap;
#[cfg(target_os = "windows")]
use std::ffi::{CStr, CString};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex as TokioMutex;
use tokio::time::sleep;

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
    message: String,
    #[serde(default)]
    login_server: String,
    #[serde(default)]
    auth_key: String,
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    config_text: String,
    /// Optional direct overlay endpoint advertised by website-api.
    #[serde(default)]
    nodepool_grpc_endpoint: String,
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
    use std::os::raw::{c_char, c_int};
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
}

#[cfg(target_os = "windows")]
use libtailscale_ffi::{
    tailscale_close, tailscale_errmsg, tailscale_getips, tailscale_listen_forward,
    tailscale_loopback, tailscale_new, tailscale_set_authkey, tailscale_set_control_url,
    tailscale_set_dir, tailscale_set_hostname, tailscale_up,
};

impl VpnSession {
    /// Get the bridge endpoint for gRPC forwarding
    pub fn bridge_endpoint(&self) -> Option<String> {
        self.bridge_addr.map(|addr| addr.to_string())
    }
}

/// Global VPN session storage
static VPN_SESSIONS: OnceLock<StdMutex<HashMap<ClientRole, Arc<VpnSession>>>> = OnceLock::new();

fn sessions_map() -> &'static StdMutex<HashMap<ClientRole, Arc<VpnSession>>> {
    VPN_SESSIONS.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Store a VPN session
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

/// Best-effort startup bootstrap when an operator already provisioned an auth key.
///
/// This is intentionally a no-op for typical downloaded clients. Those obtain a
/// preauth key automatically during login via website-api.
pub async fn ensure_env_vpn(config: &HivemindConfig, role: ClientRole) -> Result<()> {
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
            Ok(())
        }
        VpnBootstrapPlan::Join {
            auth_key,
            login_server,
            hostname,
        } => {
            join_and_confirm_nodepool(
                role,
                &auth_key,
                &login_server,
                &hostname,
                &resolve_nodepool_grpc_endpoint(config),
            )
            .await?;
            Ok(())
        }
    }
}

/// Automatic VPN join for a logged-in user using the official website-api.
///
/// Flow:
/// 1. Prefer the JWT already returned by nodepool when reachable.
/// 2. Otherwise login to website-api with the same credentials.
/// 3. POST `/api/vpn/config` to obtain WireGuard config (login_server + auth_key with embedded WireGuard keys).
/// 4. Join WireGuard VPN and return.
///
/// Returns `Ok(None)` only when website-api bootstrap is explicitly disabled.
pub async fn ensure_user_vpn(
    config: &HivemindConfig,
    role: ClientRole,
    username: &str,
    password: &str,
    existing_token: Option<&str>,
) -> Result<Option<String>> {
    let configured_endpoint = resolve_nodepool_grpc_endpoint(config);

    // Explicit env key still wins and does not need website-api.
    if env_auth_key_present(role) {
        ensure_env_vpn(config, role).await?;
        let endpoint = resolve_reachable_nodepool_endpoint(role, &configured_endpoint).await?;
        wait_for_nodepool_endpoint(&endpoint).await;
        return Ok(Some(endpoint));
    }

    let Some(website_base) = website_api_base(config, role) else {
        tracing::debug!(
            "{} website-api base disabled; skipping automatic website VPN bootstrap",
            role.as_str()
        );
        return Ok(None);
    };

    // If the overlay control plane is already reachable, do not re-issue keys.
    if let Some(endpoint) = first_reachable_nodepool_endpoint(role, &configured_endpoint).await {
        tracing::info!(
            "{} nodepool already reachable at {}; skipping VPN re-join",
            role.as_str(),
            endpoint
        );
        return Ok(Some(endpoint));
    }

    let token = match existing_token.map(str::trim).filter(|v| !v.is_empty()) {
        Some(token) => token.to_string(),
        None => website_login(&website_base, username, password).await?,
    };

    let hostname = first_nonempty(&[
        env_trim(&format!("{}_VPN_HOSTNAME", role.env_prefix())),
        env_trim("HOSTNAME"),
        env_trim("COMPUTERNAME"),
        Some(format!("{}-{}", role.as_str(), sanitize_hostname(username))),
    ])
    .unwrap_or_else(|| format!("{}-{}", role.as_str(), short_host_id()));

    let vpn = website_issue_vpn_config(&website_base, &token, &hostname).await?;
    let advertised_nodepool = first_nonempty(&[
        Some(vpn.nodepool_grpc_endpoint.clone()).filter(|v| !v.trim().is_empty()),
        extract_config_text_value(&vpn.config_text, "nodepool_grpc_endpoint"),
        extract_config_text_value(&vpn.config_text, "nodepool_endpoint"),
    ]);
    let configured_endpoint =
        first_nonempty(&[advertised_nodepool.clone(), Some(configured_endpoint)])
            .unwrap_or_else(|| DEFAULT_NODEPOOL_GRPC_ENDPOINT.to_string());
    let login_server = first_nonempty(&[
        Some(vpn.login_server.clone()).filter(|v| !v.trim().is_empty()),
        env_trim(&format!("{}_VPN_LOGIN_SERVER", role.env_prefix())),
        env_trim("HEADSCALE_LOGIN_SERVER"),
        Some(config.vpn.headscale_login_server.clone()).filter(|v| !v.trim().is_empty()),
        Some(config.vpn.headscale_url.clone()).filter(|v| !v.trim().is_empty()),
        Some(DEFAULT_HEADSCALE_LOGIN_SERVER.to_string()),
    ])
    .ok_or_else(|| {
        anyhow::anyhow!(
            "website-api VPN config did not include login_server and no HEADSCALE_LOGIN_SERVER fallback is set"
        )
    })?;

    if vpn.auth_key.trim().is_empty() {
        bail!(
            "website-api VPN config did not include auth_key: {}",
            vpn.message
        );
    }

    // website-api may return hierarchical client ids like `user:name:role`.
    // WireGuard hostnames must be valid, so always sanitize before joining.
    let join_hostname = sanitize_hostname(if vpn.client_id.trim().is_empty() {
        hostname.as_str()
    } else {
        vpn.client_id.trim()
    });

    let endpoint = join_and_confirm_nodepool(
        role,
        vpn.auth_key.trim(),
        login_server.trim_end_matches('/'),
        &join_hostname,
        &configured_endpoint,
    )
    .await?;
    Ok(Some(endpoint))
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

/// Resolve a nodepool endpoint that is reachable after VPN join.
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
    for candidate in candidates {
        if nodepool_endpoint_reachable(&candidate).await {
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
    role: ClientRole,
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
    // userspace TUN, so we expose nodepool on a localhost forwarder.
    if let Some(session) = session {
        if let Some(bridge) = session.bridge_endpoint() {
            push_unique(bridge);
        }
    }

    // Explicit operator configuration always stays first after the bridge.
    push_unique(configured_endpoint.to_string());

    let port = endpoint_port(configured_endpoint).unwrap_or(DEFAULT_NODEPOOL_GRPC_PORT);
    let hostnames = nodepool_peer_hostnames();

    // For WireGuard, we can try to resolve the nodepool hostname if we have a tunnel
    if let Some(session) = session {
        // Try to discover peer IPs through the WireGuard tunnel
        match discover_nodepool_peer_ips_over_wireguard(session, &hostnames).await {
            Ok(ips) => {
                for ip in ips {
                    push_unique(format_host_port(&ip, port));
                }
            }
            Err(err) => {
                tracing::debug!(
                    "{} WireGuard peer discovery unavailable: {err}",
                    role.as_str()
                );
            }
        }
    } else {
        // Try DNS resolution as fallback
        for hostname in &hostnames {
            match tokio::net::lookup_host(format!("{}:{}", hostname, port)).await {
                Ok(addrs) => {
                    for addr in addrs {
                        push_unique(addr.to_string());
                    }
                }
                Err(_) => {}
            }
        }
    }

    // MagicDNS-style names as a last resort when peer IP parsing is unavailable.
    for hostname in &hostnames {
        push_unique(format!("{hostname}:{port}"));
        push_unique(format!("{hostname}.hivemind.local:{port}"));
    }

    // Keep the historical VIP around for older deployments that pin it.
    push_unique(DEFAULT_NODEPOOL_GRPC_ENDPOINT.to_string());
    candidates
}

fn nodepool_peer_hostnames() -> Vec<String> {
    let mut names = Vec::new();
    if let Some(name) = env_trim("NODEPOOL_VPN_HOSTNAME") {
        names.push(name);
    }
    names.push(DEFAULT_NODEPOOL_VPN_HOSTNAME.to_string());
    names.push("nodepool".to_string());
    names
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
    let client = reqwest::Client::new();
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

async fn website_issue_vpn_config(
    base: &str,
    token: &str,
    client_name: &str,
) -> Result<WebsiteVpnConfigResponse> {
    let client = reqwest::Client::new();
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
        format!(
            "website-api VPN config returned HTTP {} with invalid JSON: {}",
            status,
            truncate_response_body(&raw)
        )
    })?;
    if !status.is_success() || !body.success {
        bail!("website-api VPN config failed: {}", body.message);
    }
    Ok(body)
}

fn truncate_response_body(body: &str) -> String {
    let compact = body.trim().replace(['\r', '\n'], " ");
    if compact.len() > 240 {
        format!("{}…", &compact[..240])
    } else {
        compact
    }
}

fn extract_config_text_value(config_text: &str, key: &str) -> Option<String> {
    for line in config_text.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix(&format!("# {key}=")) {
            return Some(val.trim().to_string());
        }
    }
    None
}

async fn join_and_confirm_nodepool(
    role: ClientRole,
    auth_key: &str,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
) -> Result<String> {
    let mut last_err = None;
    for attempt in 1..=6 {
        match bring_up_vpn(role, auth_key, login_server, hostname).await {
            Ok(session) => {
                match wait_for_nodepool_after_join(role, session.as_ref(), configured_endpoint)
                    .await
                {
                    Ok(endpoint) => {
                        spawn_vpn_keepalive(
                            role,
                            auth_key,
                            login_server,
                            hostname,
                            configured_endpoint,
                        );
                        return Ok(endpoint);
                    }
                    Err(err) => {
                        tracing::warn!(
                            "{} nodepool still unreachable after VPN join attempt {attempt}/6: {err}",
                            role.as_str()
                        );
                        last_err = Some(err);
                    }
                }
            }
            Err(err) => {
                tracing::warn!(
                    "{} VPN join attempt {attempt}/6 failed: {err}",
                    role.as_str()
                );
                last_err = Some(err);
            }
        }
        sleep(Duration::from_secs(2)).await;
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("VPN/nodepool bootstrap failed")))
}

fn spawn_vpn_keepalive(
    role: ClientRole,
    auth_key: &str,
    login_server: &str,
    hostname: &str,
    configured_endpoint: &str,
) {
    let auth_key = auth_key.to_string();
    let login_server = login_server.to_string();
    let hostname = hostname.to_string();
    let configured_endpoint = configured_endpoint.to_string();
    tokio::spawn(async move {
        vpn_keepalive_loop(role, auth_key, login_server, hostname, configured_endpoint).await;
    });
}

async fn vpn_keepalive_loop(
    role: ClientRole,
    auth_key: String,
    login_server: String,
    hostname: String,
    configured_endpoint: String,
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
                let _ = bring_up_vpn(role, &auth_key, &login_server, &hostname).await;
                continue;
            }
        };

        // Check WireGuard tunnel connectivity
        let ping_ok = wireguard_is_up(session.as_ref()).await.unwrap_or(false);
        let endpoint_ok = first_reachable_nodepool_endpoint(role, &configured_endpoint)
            .await
            .is_some();

        if ping_ok || endpoint_ok {
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
        tracing::warn!(
            "{} VPN keepalive missed nodepool (streak={failures}); forcing reconnect",
            role.as_str()
        );
        if let Err(err) = bring_up_vpn(role, &auth_key, &login_server, &hostname).await {
            tracing::warn!("{} VPN reconnect failed: {err}", role.as_str());
            continue;
        }
        let _ = wait_for_nodepool_after_join(
            role,
            current_vpn_session(role)
                .await
                .as_deref()
                .unwrap_or(session.as_ref()),
            &configured_endpoint,
        )
        .await;
    }
}

async fn wait_for_nodepool_after_join(
    role: ClientRole,
    session: &VpnSession,
    configured_endpoint: &str,
) -> Result<String> {
    let mut last_err = None;
    for attempt in 1..=40 {
        if attempt == 1 {
            tracing::info!(
                "{} probing nodepool TCP/gRPC reachability after VPN join (configured endpoint: {})",
                role.as_str(),
                configured_endpoint
            );
        }
        // Check WireGuard tunnel is up
        if attempt == 1 || attempt % 4 == 0 {
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
                    "{} nodepool connectivity probe succeeded: {}",
                    role.as_str(),
                    endpoint
                );
                return Ok(endpoint);
            }
            None => {
                last_err = Some(anyhow::anyhow!("no candidate accepted TCP"));
                // Every few failures, re-run VPN up to heal flaky joins.
                if attempt % 8 == 0 {
                    tracing::warn!(
                        "{} nodepool still down after {attempt} probes; re-running VPN up",
                        role.as_str()
                    );
                    let _ = bring_up_vpn(
                        role,
                        &session.auth_key,
                        &session.login_server,
                        &session.hostname,
                    )
                    .await;
                }
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    let candidates = nodepool_endpoint_candidates(role, configured_endpoint, Some(session)).await;
    bail!(
        "nodepool endpoint is still unreachable after VPN bootstrap (tried: {}). Check that WireGuard is connected and that the platform nodepool VPN sidecar ({}) is online{}",
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
    auth_key: &str,
    login_server: &str,
    hostname: &str,
) -> Result<Arc<VpnSession>> {
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
    #[cfg(not(target_os = "windows"))]
    let (loopback_addr, proxy_cred): (String, String) =
        { bail!("embedded libtailscale is currently only packaged for Windows") };
    #[cfg(not(target_os = "windows"))]
    let overlay_ip: Option<String> = None;
    #[cfg(target_os = "windows")]
    let network = CString::new("tcp")?;
    #[cfg(target_os = "windows")]
    let tailnet_addr = CString::new(format!(":{}", endpoint_port_for_worker(role)))?;
    #[cfg(target_os = "windows")]
    let local_addr = CString::new(format!("127.0.0.1:{}", endpoint_port_for_worker(role)))?;
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
    let configured_endpoint = std::env::var("NODEPOOL_GRPC_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_NODEPOOL_GRPC_ENDPOINT.to_string());
    tracing::info!(
        "{} VPN joined via embedded libtailscale; probing nodepool {}",
        role.as_str(),
        configured_endpoint
    );
    let bridge_addr = start_socks_bridge(&loopback_addr, &proxy_cred, &configured_endpoint).await?;

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
        auth_key: auth_key.to_string(),
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
        return Ok(start_socks_bridge(socks, cred, target).await?.to_string());
    }
    Ok(target.to_string())
}

fn resolve_tailscale_bins(role: ClientRole) -> Result<(PathBuf, PathBuf)> {
    let mut roots = Vec::new();
    if let Ok(root) = std::env::var("HIVEMIND_CLIENT_RUNTIME_DIR") {
        roots.push(PathBuf::from(root));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.join("vpn"));
            roots.push(parent.to_path_buf());
        }
    }
    roots.push(vpn_state_dir(role));

    let exe_name = if cfg!(target_os = "windows") {
        "tailscale.exe"
    } else {
        "tailscale"
    };
    let daemon_name = if cfg!(target_os = "windows") {
        "tailscaled.exe"
    } else {
        "tailscaled"
    };
    for root in roots {
        let cli = root.join(exe_name);
        let daemon = root.join(daemon_name);
        if cli.is_file() && daemon.is_file() {
            return Ok((cli, daemon));
        }
    }
    let cli = PathBuf::from(exe_name);
    let daemon = PathBuf::from(daemon_name);
    if which_on_path(&cli) && which_on_path(&daemon) {
        return Ok((cli, daemon));
    }
    bail!("bundled Tailscale runtime not found; expected {exe_name} and {daemon_name} beside the client")
}

fn endpoint_port_for_worker(_role: ClientRole) -> u16 {
    50053
}

#[cfg(target_os = "windows")]
async fn start_libtailscale(
    state_dir: &PathBuf,
    hostname: &str,
    auth_key: &str,
    login_server: &str,
) -> Result<(Arc<LibtailscaleSession>, String, String, Option<String>)> {
    let state_dir = state_dir.clone();
    let hostname = CString::new(hostname)?;
    let auth_key = CString::new(auth_key)?;
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
            (
                tailscale_set_authkey(handle, auth_key.as_ptr()),
                "set auth key",
            ),
            (
                tailscale_set_control_url(handle, login_server.as_ptr()),
                "set control URL",
            ),
        ] {
            if ok != 0 {
                let err = fail(name);
                tailscale_close(handle);
                return Err(err);
            }
        }
        tracing::info!(
            "worker embedded libtailscale starting Headscale login at {}",
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

fn which_on_path(program: &PathBuf) -> bool {
    if program.components().count() > 1 {
        return program.is_file();
    }
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

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
    let (host, port) = target
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid nodepool endpoint: {target}"))?;
    let port: u16 = port.parse()?;
    let ip = host.parse::<Ipv4Addr>();
    let mut request = vec![5, 1, 0];
    if let Ok(ip) = ip {
        request.push(1);
        request.extend_from_slice(&ip.octets());
    } else {
        request.push(3);
        request.push(host.len().try_into()?);
        request.extend_from_slice(host.as_bytes());
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

/// Discover nodepool peer IPs over WireGuard tunnel
async fn discover_nodepool_peer_ips_over_wireguard(
    session: &VpnSession,
    hostnames: &[String],
) -> Result<Vec<String>> {
    // For WireGuard, we can try to resolve via the tunnel or use DNS
    // Since we don't have a full TUN device, we'll try DNS resolution
    // as the WireGuard tunnel provides the network connectivity
    let mut ips = Vec::new();
    for hostname in hostnames {
        // Try to resolve via the WireGuard network
        // We can attempt to connect to the endpoint and extract the peer IP
        if let Some(endpoint) = session.wg_endpoint {
            // Try DNS resolution through the tunnel context
            match tokio::net::lookup_host(format!("{}:{}", hostname, DEFAULT_NODEPOOL_GRPC_PORT))
                .await
            {
                Ok(addrs) => {
                    for addr in addrs {
                        ips.push(addr.ip().to_string());
                    }
                }
                Err(_) => {}
            }
        }
    }
    Ok(ips)
}

async fn wait_for_nodepool_endpoint(endpoint: &str) {
    for _ in 0..30 {
        if nodepool_endpoint_reachable(endpoint).await {
            return;
        }
        sleep(Duration::from_millis(500)).await;
    }
}

/// Check if a nodepool endpoint is reachable via TCP
async fn nodepool_endpoint_reachable(endpoint: &str) -> bool {
    tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(endpoint))
        .await
        .is_ok_and(|r| r.is_ok())
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

fn format_host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

fn endpoint_port(endpoint: &str) -> Option<u16> {
    let endpoint = endpoint.trim();
    let endpoint = endpoint
        .strip_prefix("http://")
        .or_else(|| endpoint.strip_prefix("https://"))
        .unwrap_or(endpoint);
    if endpoint.starts_with('[') {
        endpoint.split("]:").nth(1)?.parse().ok()
    } else {
        endpoint.rsplit_once(':')?.1.parse().ok()
    }
}

/// Ensure a userspace TCP bridge for gRPC over WireGuard
async fn ensure_userspace_bridge(session: &VpnSession, peer_ip: &str) -> Result<()> {
    if let Some(bridge_addr) = session.bridge_addr {
        // Bridge already running on the WireGuard local address
        // The WireGuard tunnel already provides the network path
        return Ok(());
    }
    Ok(())
}

/// Get the VPN state directory for a role
fn vpn_state_dir(role: ClientRole) -> PathBuf {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".hivemind")
        .join(format!("{}-vpn", role.as_str()))
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
    for opt in options {
        if let Some(val) = opt {
            if !val.is_empty() {
                return Some(val.clone());
            }
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
            let peer_public_key = config.peers[0].public_key.clone();
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
                                            Ok(packet) => {
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
    pub async fn build_wireguard_config_from_vpn(
        vpn_config: &WebsiteVpnConfigResponse,
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
}
