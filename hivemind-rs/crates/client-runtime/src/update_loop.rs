//! Runtime orchestration for the signed client update protocol.
//!
//! This module deliberately has no unsigned or unverified path. A missing
//! endpoint, missing approved host, invalid signature, failed download, or
//! failed installation leaves the currently running client untouched and is
//! reported as deferred/failed for the next bounded retry.

use crate::update::{
    download_signed_metadata, download_verified_package, extract_verified_zip, parse_signed_keyset,
    parse_signed_manifest, UpdateError, UpdateInstaller, UpdatePolicy, UpdateVerifier,
    UPDATE_KEYSET_MAX_BYTES, UPDATE_MANIFEST_MAX_BYTES,
};
use crate::ClientRole;
use hivemind_config::HivemindConfig;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tracing::{info, warn};

const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone)]
pub struct UpdateLoopConfig {
    pub role: ClientRole,
    pub policy: UpdatePolicy,
    pub install_root: PathBuf,
    pub staging_root: PathBuf,
    pub keyset_url: String,
    pub manifest_url: String,
    pub interval: Duration,
    pub max_backoff: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCycleOutcome {
    Disabled,
    Deferred(String),
    UpToDate { sequence: u64 },
    Installed { sequence: u64, version: String },
}

/// Translate the shared configuration into a validated Windows update loop.
///
/// A missing signed metadata endpoint is an explicit deferred state. Once an
/// endpoint is configured, the approved host list, HTTPS URLs, product, and
/// operator-owned absolute storage roots are mandatory; malformed values fail
/// closed before any background task is spawned.
pub fn config_for_role(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<Option<UpdateLoopConfig>, UpdateError> {
    let settings = &config.client_updates;
    settings.validate().map_err(UpdateError::InvalidMetadata)?;
    if !settings.enabled {
        return Ok(None);
    }
    if !cfg!(target_os = "windows") {
        return Ok(None);
    }

    let keyset_url = match settings.keyset_url.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => return Ok(None),
    };
    let manifest_url = match settings.manifest_url.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => value.to_owned(),
        _ => return Ok(None),
    };
    if settings.allowed_hosts.is_empty() {
        return Err(UpdateError::InvalidMetadata(
            "signed update approved host list is not configured".into(),
        ));
    }

    let expected_product = default_product(role);
    let product = settings
        .product
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(expected_product);
    if product != expected_product {
        return Err(UpdateError::TargetMismatch);
    }
    let channel = settings.channel.trim();
    if channel.is_empty() {
        return Err(UpdateError::InvalidMetadata(
            "signed update channel is blank".into(),
        ));
    }

    let host_refs: Vec<&str> = settings.allowed_hosts.iter().map(String::as_str).collect();
    let policy = UpdatePolicy::new(
        product,
        channel,
        "windows",
        current_windows_architecture(),
        &host_refs,
    )
    .with_max_package_bytes(settings.max_package_bytes);
    if policy.allowed_hosts.is_empty() {
        return Err(UpdateError::InvalidMetadata(
            "signed update approved host list is empty".into(),
        ));
    }
    crate::update::validate_https_package_url(&keyset_url, &policy.allowed_hosts)?;
    crate::update::validate_https_package_url(&manifest_url, &policy.allowed_hosts)?;

    let install_root = resolve_storage_root(settings.install_root.as_deref(), role, "installed")?;
    let staging_root = match settings.staging_root.as_deref().map(str::trim) {
        Some(value) if !value.is_empty() => resolve_absolute_path(value)?,
        _ => install_root.join("staging"),
    };
    let interval = Duration::from_secs(settings.check_interval_secs);
    if interval.is_zero() {
        return Err(UpdateError::InvalidMetadata(
            "signed update check interval is zero".into(),
        ));
    }

    Ok(Some(UpdateLoopConfig {
        role,
        policy,
        install_root,
        staging_root,
        keyset_url,
        manifest_url,
        interval,
        max_backoff: MAX_RETRY_BACKOFF,
    }))
}

/// Run one fail-closed update check for a client role.
///
/// Disabled clients and clients without a signed endpoint remain explicit
/// `Disabled`/`Deferred` states. No unsigned or unapproved source is selected.
pub async fn run_update_cycle(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<UpdateCycleOutcome, UpdateError> {
    if !config.client_updates.enabled {
        return Ok(UpdateCycleOutcome::Disabled);
    }
    if !cfg!(target_os = "windows") {
        return Ok(UpdateCycleOutcome::Deferred(
            "signed client updates are configured only for native Windows clients".into(),
        ));
    }
    let update_config = match config_for_role(config, role)? {
        Some(update_config) => update_config,
        None => {
            return Ok(UpdateCycleOutcome::Deferred(
                "signed update endpoints are not configured".into(),
            ))
        }
    };
    run_configured_update_cycle(&update_config).await
}

/// Execute a validated update configuration. This private boundary keeps
/// endpoint and storage validation ahead of all filesystem/network activity.
async fn run_configured_update_cycle(
    config: &UpdateLoopConfig,
) -> Result<UpdateCycleOutcome, UpdateError> {
    let installer = UpdateInstaller::new(&config.install_root)?;
    let current = installer.load_state()?;
    let verifier = UpdateVerifier::embedded()?;
    let now_unix = unix_now()?;

    let keyset_bytes = download_signed_metadata(
        &config.keyset_url,
        UPDATE_KEYSET_MAX_BYTES,
        &config.policy.allowed_hosts,
    )
    .await?;
    let signed_keyset = parse_signed_keyset(&keyset_bytes)?;
    let verified_keyset = verifier.verify_keyset(&signed_keyset, &config.policy, now_unix)?;

    let manifest_bytes = download_signed_metadata(
        &config.manifest_url,
        UPDATE_MANIFEST_MAX_BYTES,
        &config.policy.allowed_hosts,
    )
    .await?;
    let signed_manifest = parse_signed_manifest(&manifest_bytes)?;

    if let Some(current) = current.as_ref() {
        if signed_manifest.manifest.sequence == current.current.sequence {
            let state = installer.verify_current(
                &signed_manifest,
                &verified_keyset,
                &verifier,
                &config.policy,
                now_unix,
            )?;
            return Ok(UpdateCycleOutcome::UpToDate {
                sequence: state.current.sequence,
            });
        }
    }

    let verified_manifest = verifier.verify_manifest(
        &signed_manifest,
        &verified_keyset,
        &config.policy,
        current.as_ref(),
        now_unix,
    )?;
    let package_staging = config.staging_root.join("packages");
    let package_path =
        download_verified_package(&verified_manifest, &config.policy, &package_staging).await?;
    let extracted_dir = config.staging_root.join(format!(
        ".release-{}-{}-{}",
        verified_manifest.manifest().sequence,
        &verified_manifest.manifest().package_sha256[..16],
        monotonic_nonce()
    ));

    let extraction_result = extract_verified_zip(
        &package_path,
        &extracted_dir,
        &verified_manifest,
        &config.policy,
    );
    if let Err(error) = extraction_result {
        let _ = std::fs::remove_dir_all(&extracted_dir);
        return Err(error);
    }

    let install_result = installer.install_verified_directory(
        &extracted_dir,
        &signed_manifest,
        &verified_keyset,
        &verifier,
        &config.policy,
        now_unix,
    );
    let cleanup_result = std::fs::remove_dir_all(&extracted_dir);
    let installed = match (install_result, cleanup_result) {
        (Ok(installed), Ok(())) => installed,
        (Ok(_), Err(error)) => {
            return Err(UpdateError::Io(format!(
                "verified update was installed but staging cleanup failed: {error}"
            )))
        }
        (Err(error), Ok(())) => return Err(error),
        (Err(error), Err(cleanup_error)) => {
            return Err(UpdateError::Io(format!(
            "verified update installation failed: {error}; staging cleanup failed: {cleanup_error}"
        )))
        }
    };

    Ok(UpdateCycleOutcome::Installed {
        sequence: installed.current.sequence,
        version: installed.current.version,
    })
}

/// Start the signed update loop for a client role and return its shutdown
/// signal. Missing signed endpoints intentionally remain deferred; they do not
/// cause a network request and never select an unsigned source.
pub fn start_update_loop(config: HivemindConfig, role: ClientRole) -> watch::Sender<bool> {
    let interval = Duration::from_secs(config.client_updates.check_interval_secs.max(1));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let mut next_wait = Duration::ZERO;
        let mut retry_backoff = Duration::from_secs(1);

        loop {
            tokio::select! {
                _ = tokio::time::sleep(next_wait) => {
                    match run_update_cycle(&config, role).await {
                        Ok(UpdateCycleOutcome::Disabled) => {
                            info!(role = role_name(role), "Signed client updates are disabled");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::Deferred(reason)) => {
                            info!(role = role_name(role), reason = %reason, "Signed client updates are deferred");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::UpToDate { sequence }) => {
                            info!(role = role_name(role), sequence, "Signed client update is already installed");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::Installed { sequence, version }) => {
                            info!(role = role_name(role), sequence, version = %version, "Signed client release verified and staged; restart is required to activate it");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Err(error) => {
                            warn!(role = role_name(role), error = %error, "Signed client update check failed; keeping the current client and retrying");
                            next_wait = retry_backoff;
                            retry_backoff = retry_backoff
                                .checked_mul(2)
                                .unwrap_or(MAX_RETRY_BACKOFF)
                                .min(MAX_RETRY_BACKOFF);
                        }
                    }
                }
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        info!(role = role_name(role), "Signed client update loop shutting down");
                        break;
                    }
                }
            }
        }
    });
    shutdown_tx
}

fn default_product(role: ClientRole) -> &'static str {
    match role {
        ClientRole::Master => "hivemind-windows-master",
        ClientRole::Worker => "hivemind-windows-worker",
    }
}

fn role_name(role: ClientRole) -> &'static str {
    match role {
        ClientRole::Master => "master",
        ClientRole::Worker => "worker",
    }
}

fn current_windows_architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "unknown"
    }
}

fn resolve_storage_root(
    configured: Option<&str>,
    role: ClientRole,
    kind: &str,
) -> Result<PathBuf, UpdateError> {
    match configured.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => resolve_absolute_path(value),
        None => {
            let base = dirs::data_local_dir()
                .or_else(dirs::data_dir)
                .or_else(dirs::home_dir)
                .ok_or_else(|| {
                    UpdateError::Io("could not determine an operator-owned update root".into())
                })?;
            Ok(base
                .join("hivemind")
                .join(format!("{}-{kind}", role_name(role))))
        }
    }
}

fn resolve_absolute_path(value: &str) -> Result<PathBuf, UpdateError> {
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(UpdateError::UnsafePath(value.to_owned()));
    }
    Ok(path.to_path_buf())
}

fn unix_now() -> Result<u64, UpdateError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| UpdateError::Io(error.to_string()))
}

fn monotonic_nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_updates_do_not_require_network_configuration() {
        let mut config = HivemindConfig::for_test();
        config.client_updates.enabled = false;
        assert!(config_for_role(&config, ClientRole::Worker)
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn missing_signed_endpoint_is_deferred_without_installing() {
        let mut config = HivemindConfig::for_test();
        config.client_updates.enabled = true;
        config.client_updates.keyset_url = None;
        config.client_updates.manifest_url = None;
        if cfg!(target_os = "windows") {
            assert!(config_for_role(&config, ClientRole::Worker)
                .unwrap()
                .is_none());
        }
    }

    #[test]
    fn default_products_are_role_specific() {
        assert_eq!(
            default_product(ClientRole::Worker),
            "hivemind-windows-worker"
        );
        assert_eq!(
            default_product(ClientRole::Master),
            "hivemind-windows-master"
        );
    }

    #[test]
    fn relative_storage_roots_are_rejected() {
        assert!(matches!(
            resolve_absolute_path("relative/update-root"),
            Err(UpdateError::UnsafePath(_))
        ));
    }
}
