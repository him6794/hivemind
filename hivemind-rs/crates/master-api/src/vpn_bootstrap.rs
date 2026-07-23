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

/// Best-effort startup bootstrap when an operator already provisioned an auth key.
pub async fn ensure_master_vpn(config: &HivemindConfig) -> Result<()> {
    client_runtime::ensure_env_vpn(config, ClientRole::Master).await
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

pub fn website_api_base(config: &HivemindConfig) -> Option<String> {
    client_runtime::website_api_base(config, ClientRole::Master)
}
