//! Runtime orchestration for the signed client update protocol.
//!
//! This module deliberately has no unsigned or unverified path. A missing
//! endpoint, missing approved host, invalid signature, failed download, or
//! failed installation leaves the currently running client untouched and is
//! reported as deferred/failed for the next bounded retry.

use crate::update::{
    active_signed_metadata_fingerprint, download_signed_metadata, download_verified_package_cached,
    extract_verified_zip, installed_tree_metadata_fingerprint, parse_signed_keyset,
    parse_signed_manifest, release_matches_manifest, spawn_activation_helper,
    PackageVerificationCache, UpdateError, UpdateInstaller, UpdatePolicy, UpdateVerifier,
    VerifiedReleaseManifest, UPDATE_FULL_REVERIFY_INTERVAL_SECS, UPDATE_KEYSET_MAX_BYTES,
    UPDATE_MANIFEST_MAX_BYTES,
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
    pub policy: UpdatePolicy,
    pub install_root: PathBuf,
    pub staging_root: PathBuf,
    pub keyset_url: String,
    pub manifest_url: String,
    pub service_arguments: Vec<String>,
}

#[derive(Debug, Default)]
struct UpdateVerificationCache {
    installed: Option<InstalledVerificationCache>,
    active: Option<ActiveVerificationCache>,
    package: Option<PackageVerificationCache>,
}

#[derive(Debug)]
struct InstalledVerificationCache {
    release_dir: String,
    manifest_sha256: String,
    metadata_fingerprint: String,
    verified_at_unix: u64,
}

#[derive(Debug)]
struct ActiveVerificationCache {
    root: PathBuf,
    manifest_sha256: String,
    metadata_fingerprint: String,
    verified_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCycleOutcome {
    Disabled,
    Deferred(String),
    UpToDate {
        sequence: u64,
    },
    Activating {
        sequence: u64,
        version: String,
        descriptor: PathBuf,
    },
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
        crate::update::current_windows_architecture(),
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
    if settings.check_interval_secs == 0 {
        return Err(UpdateError::InvalidMetadata(
            "signed update check interval is zero".into(),
        ));
    }

    Ok(Some(UpdateLoopConfig {
        policy,
        install_root,
        staging_root,
        keyset_url,
        manifest_url,
        service_arguments: current_service_arguments(),
    }))
}

/// Clean helper copies left by a completed activation before normal service
/// startup. Pending descriptors are intentionally preserved for recovery.
pub fn cleanup_activation_helpers_for_role(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<usize, UpdateError> {
    let Some(update_config) = config_for_role(config, role)? else {
        return Ok(0);
    };
    UpdateInstaller::new(update_config.install_root)?.cleanup_activation_helpers()
}

/// Run one fail-closed update check for a client role.
///
/// Disabled clients and clients without a signed endpoint remain explicit
/// `Disabled`/`Deferred` states. No unsigned or unapproved source is selected.
pub async fn run_update_cycle(
    config: &HivemindConfig,
    role: ClientRole,
) -> Result<UpdateCycleOutcome, UpdateError> {
    let mut cache = UpdateVerificationCache::default();
    run_update_cycle_with_arguments(config, role, &current_service_arguments(), &mut cache).await
}

async fn run_update_cycle_with_arguments(
    config: &HivemindConfig,
    role: ClientRole,
    service_arguments: &[String],
    cache: &mut UpdateVerificationCache,
) -> Result<UpdateCycleOutcome, UpdateError> {
    if !config.client_updates.enabled {
        return Ok(UpdateCycleOutcome::Disabled);
    }
    if !cfg!(target_os = "windows") {
        return Ok(UpdateCycleOutcome::Deferred(
            "signed client updates are configured only for native Windows clients".into(),
        ));
    }
    let mut update_config = match config_for_role(config, role)? {
        Some(update_config) => update_config,
        None => {
            return Ok(UpdateCycleOutcome::Deferred(
                "signed update endpoints are not configured".into(),
            ))
        }
    };
    update_config.service_arguments = service_arguments.to_vec();
    run_configured_update_cycle(&update_config, cache).await
}

/// Execute a validated update configuration. This private boundary keeps
/// endpoint and storage validation ahead of all filesystem/network activity.
async fn run_configured_update_cycle(
    config: &UpdateLoopConfig,
    cache: &mut UpdateVerificationCache,
) -> Result<UpdateCycleOutcome, UpdateError> {
    let installer = UpdateInstaller::new(&config.install_root)?;
    let current = installer.load_state()?;
    if let Some(descriptor) = installer.pending_activation_path()? {
        let current = current.as_ref().ok_or(UpdateError::MissingState)?;
        spawn_activation_helper(&descriptor)?;
        return Ok(UpdateCycleOutcome::Activating {
            sequence: current.current.sequence,
            version: current.current.version.clone(),
            descriptor,
        });
    }
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
            let verified_current = verifier.verify_manifest(
                &signed_manifest,
                &verified_keyset,
                &config.policy,
                None,
                now_unix,
            )?;
            let state = verify_installed_current_cached(
                &installer,
                current,
                &verified_current,
                &config.policy,
                cache,
                now_unix,
            )?;
            if active_package_is_current_cached(&verified_current, cache, now_unix)? {
                let state = if state.last_known_good.as_ref() == Some(&state.current) {
                    state
                } else {
                    installer.promote_current_verified(state)?
                };
                if let Err(error) = installer.prune_old_releases() {
                    warn!(error = %error, "verified update release eviction failed; retrying later");
                }
                if let Err(error) = installer.cleanup_activation_helpers() {
                    warn!(error = %error, "completed update helper cleanup failed; retrying later");
                }
                return Ok(UpdateCycleOutcome::UpToDate {
                    sequence: state.current.sequence,
                });
            }
            let descriptor = installer.prepare_activation_from_current_process(
                &signed_keyset,
                &signed_manifest,
                &verifier,
                &config.policy,
                now_unix,
                &config.service_arguments,
            )?;
            spawn_activation_helper(&descriptor)?;
            return Ok(UpdateCycleOutcome::Activating {
                sequence: state.current.sequence,
                version: state.current.version,
                descriptor,
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
    let package_path = download_verified_package_cached(
        &verified_manifest,
        &config.policy,
        &package_staging,
        &mut cache.package,
        now_unix,
    )
    .await?;
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

    let descriptor = installer.prepare_activation_from_current_process(
        &signed_keyset,
        &signed_manifest,
        &verifier,
        &config.policy,
        now_unix,
        &config.service_arguments,
    )?;
    spawn_activation_helper(&descriptor)?;

    Ok(UpdateCycleOutcome::Activating {
        sequence: installed.current.sequence,
        version: installed.current.version,
        descriptor,
    })
}

/// Start the signed update loop for a client role and return its shutdown
/// signal. Missing signed endpoints intentionally remain deferred; they do not
/// cause a network request and never select an unsigned source.
pub fn start_update_loop(
    config: HivemindConfig,
    role: ClientRole,
    service_arguments: Vec<String>,
    activation_tx: watch::Sender<bool>,
) -> watch::Sender<bool> {
    let interval = Duration::from_secs(config.client_updates.check_interval_secs.max(1));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let mut next_wait = Duration::ZERO;
        let mut retry_backoff = Duration::from_secs(1);
        let mut verification_cache = UpdateVerificationCache::default();

        loop {
            tokio::select! {
                _ = tokio::time::sleep(next_wait) => {
                    match run_update_cycle_with_arguments(
                        &config,
                        role,
                        &service_arguments,
                        &mut verification_cache,
                    ).await {
                        Ok(UpdateCycleOutcome::Disabled) => {
                            info!(role = role.as_str(), "Signed client updates are disabled");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::Deferred(reason)) => {
                            info!(role = role.as_str(), reason = %reason, "Signed client updates are deferred");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::UpToDate { sequence }) => {
                            info!(role = role.as_str(), sequence, "Signed client update is already installed");
                            next_wait = interval;
                            retry_backoff = Duration::from_secs(1);
                        }
                        Ok(UpdateCycleOutcome::Activating { sequence, version, descriptor }) => {
                            info!(
                                role = role.as_str(),
                                sequence,
                                version = %version,
                                descriptor = %descriptor.display(),
                                "Signed client release verified; activation helper started"
                            );
                            let _ = activation_tx.send(true);
                            break;
                        }
                        Err(error) => {
                            warn!(role = role.as_str(), error = %error, "Signed client update check failed; keeping the current client and retrying");
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
                        info!(role = role.as_str(), "Signed client update loop shutting down");
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

fn verify_installed_current_cached(
    installer: &UpdateInstaller,
    current: &crate::update::InstalledPackageState,
    verified: &VerifiedReleaseManifest,
    policy: &UpdatePolicy,
    cache: &mut UpdateVerificationCache,
    now_unix: u64,
) -> Result<crate::update::InstalledPackageState, UpdateError> {
    if current.product != policy.product || !release_matches_manifest(&current.current, verified) {
        return Err(UpdateError::CorruptState(
            "installed state does not match the signed manifest".into(),
        ));
    }
    let release_root = installer.verified_release_path(&current.current.release_dir)?;
    let metadata_fingerprint = installed_tree_metadata_fingerprint(&release_root)?;
    let cache_hit = cache.installed.as_ref().is_some_and(|cached| {
        cached.release_dir == current.current.release_dir
            && cached.manifest_sha256 == verified.manifest_sha256()
            && cached.metadata_fingerprint == metadata_fingerprint
            && cache_is_fresh(cached.verified_at_unix, now_unix)
    });
    if !cache_hit {
        installer.verify_current_against_verified_manifest(current, verified, policy)?;
        cache.installed = Some(InstalledVerificationCache {
            release_dir: current.current.release_dir.clone(),
            manifest_sha256: verified.manifest_sha256().to_owned(),
            metadata_fingerprint,
            verified_at_unix: now_unix,
        });
    }
    Ok(current.clone())
}

fn active_package_is_current_cached(
    verified: &VerifiedReleaseManifest,
    cache: &mut UpdateVerificationCache,
    now_unix: u64,
) -> Result<bool, UpdateError> {
    let executable = std::env::current_exe()
        .map_err(|error| UpdateError::ActivationUnavailable(error.to_string()))?;
    let root = executable.parent().ok_or_else(|| {
        UpdateError::ActivationUnavailable("running executable has no parent directory".into())
    })?;
    let metadata_fingerprint = match active_signed_metadata_fingerprint(root, verified) {
        Ok(fingerprint) => fingerprint,
        Err(UpdateError::PackageMismatch) => return Ok(false),
        Err(error) => return Err(error),
    };
    if cache.active.as_ref().is_some_and(|cached| {
        cached.root == root
            && cached.manifest_sha256 == verified.manifest_sha256()
            && cached.metadata_fingerprint == metadata_fingerprint
            && cache_is_fresh(cached.verified_at_unix, now_unix)
    }) {
        return Ok(true);
    }
    match crate::update::verify_active_directory(root, verified) {
        Ok(()) => {
            cache.active = Some(ActiveVerificationCache {
                root: root.to_path_buf(),
                manifest_sha256: verified.manifest_sha256().to_owned(),
                metadata_fingerprint,
                verified_at_unix: now_unix,
            });
            Ok(true)
        }
        Err(UpdateError::PackageMismatch) => Ok(false),
        Err(error) => Err(error),
    }
}

fn cache_is_fresh(verified_at_unix: u64, now_unix: u64) -> bool {
    now_unix >= verified_at_unix
        && now_unix.saturating_sub(verified_at_unix) < UPDATE_FULL_REVERIFY_INTERVAL_SECS
}

fn current_service_arguments() -> Vec<String> {
    std::env::args().skip(1).collect()
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
                .join(format!("{}-{kind}", role.as_str())))
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
