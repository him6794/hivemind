//! Compatibility wrappers around the shared client-runtime VPN helpers.

use anyhow::Result;
use hivemind_client_runtime::{self as client_runtime, ClientRole};
use hivemind_config::HivemindConfig;

pub use hivemind_client_runtime::VpnBootstrapPlan;

/// Resolve whether master should join the platform VPN from explicit settings.
pub fn plan_master_vpn_bootstrap(
    auth_key: Option<&str>,
    login_server: Option<&str>,
    hostname: Option<&str>,
    config_login_server: Option<&str>,
) -> Result<VpnBootstrapPlan> {
    client_runtime::plan_vpn_bootstrap(
        auth_key,
        login_server,
        hostname,
        config_login_server,
        ClientRole::Master,
    )
}

/// Startup bootstrap when an operator already provisioned a role-scoped auth key.
///
/// `Some` contains the validated Nodepool endpoint, which may be a localhost
/// userspace bridge on Windows. `None` preserves the deferred UI-login mode.
pub async fn ensure_master_vpn(
    config: &HivemindConfig,
    configured_endpoint: &str,
) -> Result<Option<String>> {
    client_runtime::ensure_env_vpn_for_endpoint(config, ClientRole::Master, configured_endpoint)
        .await
}

/// Automatic VPN join for a logged-in user using the official website-api.
///
/// Returns the discovered nodepool endpoint when bootstrap made (or found) a
/// reachable overlay control plane.
pub async fn ensure_master_vpn_for_user(
    config: &HivemindConfig,
    username: &str,
    password: &str,
    existing_token: Option<&str>,
) -> Result<Option<String>> {
    client_runtime::ensure_user_vpn(
        config,
        ClientRole::Master,
        username,
        password,
        existing_token,
    )
    .await
}

/// Bootstrap an already-authenticated local session without accepting or
/// storing a password. The raw JWT is forwarded only to the protected
/// Website API by the shared runtime helper.
pub async fn ensure_master_vpn_for_token(
    config: &HivemindConfig,
    token: &str,
) -> Result<Option<String>> {
    client_runtime::ensure_user_vpn_for_token(config, ClientRole::Master, token).await
}

pub fn website_api_base(config: &HivemindConfig) -> Option<String> {
    client_runtime::website_api_base(config, ClientRole::Master)
}
