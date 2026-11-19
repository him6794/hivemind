//! Nodepool LLM control plane:
//! - Single-transaction atomic `BlockQuota` admission, reservation & settlement
//! - Trust Tiers (Tier 0 Private / Tier 1 Public / Tier 2 Confidential TEE trait)
//! - Hybrid Overflow (`PreferPrivateThenPublic` vs `StrictPrivate`) & LAN-direct preference
//! - Auto & Manual Inference Router with pre-first-token automatic failover
//! - Authenticated Nodepool-hosted `ModelSwarmTrackerService`
//! - Multi-device group metadata management & public network status summary

#![allow(clippy::result_large_err)]

use crate::grpc::NodepoolState;
use hivemind_model_catalog::{BlockQuota, Catalog, ModelEntry, QuotaError, TOKENS_PER_BLOCK};
use hivemind_model_swarm::Registry as SwarmRegistry;
use hivemind_proto::llm_inference_service_server::LlmInferenceService;
use hivemind_proto::model_swarm_tracker_service_server::ModelSwarmTrackerService;
use hivemind_proto::{
    AccountDeviceView, AnnounceModelPeerRequest, AnnounceModelPeerResponse, GetModelCatalogRequest,
    GetModelCatalogResponse, GetModelPeersRequest, GetModelPeersResponse,
    GetPublicNetworkStatusRequest, GetPublicNetworkStatusResponse, ListAccountDevicesRequest,
    ListAccountDevicesResponse, ListOnlineModelsRequest, ListOnlineModelsResponse, LlmInferChunk,
    LlmInferRequest, LlmInferResponse, OnlineModelInfo, PublicModelStatus,
    PublishModelCatalogRequest, PublishModelCatalogResponse, ReportWorkerModelStatusRequest,
    ReportWorkerModelStatusResponse, SwarmPeerInfo, UpdateDeviceMetadataRequest,
    UpdateDeviceMetadataResponse,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};

// =============================================================================
// Trust Tiers, Hybrid Overflow & Tier 2 Confidential TEE Interface
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustTier {
    /// Strict owner/team-only private compute mesh: prompts and tasks never leave
    /// devices owned by the requesting account.
    Tier0Private = 0,
    /// Public mutual-aid compute pool: prompts are visible to contributing nodes
    /// (explicitly disclosed via UI and `X-Hivemind-Data-Privacy` headers).
    Tier1Public = 1,
    /// Future hardware Confidential Computing pool (NVIDIA CC / AMD SEV / Intel TDX).
    Tier2ConfidentialTee = 2,
}

impl TrustTier {
    #[must_use]
    pub fn parse_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "tier0" | "tier0_private" | "private" => Self::Tier0Private,
            "tier2" | "tier2_tee" | "tier2_confidential_tee" | "tee" => Self::Tier2ConfidentialTee,
            _ => Self::Tier1Public,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tier0Private => "tier0_private",
            Self::Tier1Public => "tier1_public",
            Self::Tier2ConfidentialTee => "tier2_tee",
        }
    }

    #[must_use]
    pub const fn privacy_disclosure_header(self) -> &'static str {
        match self {
            Self::Tier0Private => "private-tier0",
            Self::Tier1Public => "public-unencrypted-to-worker",
            Self::Tier2ConfidentialTee => "confidential-tee",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    /// Prefer same-account private devices (LAN-direct first); automatically
    /// overflow to the public mutual-aid pool when private devices are busy or offline.
    #[default]
    PreferPrivateThenPublic,
    /// Never send prompts or tasks outside same-account devices; reject if none available.
    StrictPrivate,
}

impl OverflowPolicy {
    #[must_use]
    pub fn parse_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "strict_private" | "strict" | "private_only" => Self::StrictPrivate,
            _ => Self::PreferPrivateThenPublic,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreferPrivateThenPublic => "prefer_private_then_public",
            Self::StrictPrivate => "strict_private",
        }
    }
}

/// Hardware TEE vendor reserved for Tier 2 Confidential Computing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeeVendor {
    NvidiaConfidentialComputing,
    AmdSevSnp,
    IntelTdx,
}

/// Remote attestation evidence presented by a Tier 2 TEE worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeeAttestationReport {
    pub vendor: TeeVendor,
    pub measurement_hex: String,
    pub quote_bytes: Vec<u8>,
    pub worker_ephemeral_pubkey_hex: String,
    pub nonce_hex: String,
}

/// Session key wrapped to a verified TEE enclave's public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeeKeyDeliveryEnvelope {
    pub worker_id: String,
    pub vendor: TeeVendor,
    pub wrapped_session_key: Vec<u8>,
    pub key_id: String,
}

/// Reserved remote attestation & encrypted key delivery interface for Tier 2 TEE.
pub trait TeeAttestationVerifier: Send + Sync {
    fn verify_attestation(&self, report: &TeeAttestationReport) -> Result<(), String>;
    fn wrap_session_key(
        &self,
        worker_id: &str,
        report: &TeeAttestationReport,
        session_key: &[u8; 32],
    ) -> Result<TeeKeyDeliveryEnvelope, String>;
}

// =============================================================================
// Atomic Single-Transaction BlockQuota Admission & Settlement
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedLlmRequest {
    pub request_id: String,
    pub owner: String,
    pub model_id: String,
    pub reserved_input_tokens: u64,
    pub reserved_output_tokens: u64,
    pub input_blocks_charged: u64,
    pub output_blocks_charged: u64,
    pub charged_cpt: i64,
    pub remaining_balance_cpt: i64,
    pub input_available_after: u64,
    pub output_available_after: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledLlmRequest {
    pub request_id: String,
    pub actual_input_tokens: u64,
    pub actual_output_tokens: u64,
    pub refunded_input_tokens: u64,
    pub refunded_output_tokens: u64,
    pub input_available_after: u64,
    pub output_available_after: u64,
    pub worker_credited_cpt: i64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmBillingError {
    #[error("account `{0}` not found")]
    UserNotFound(String),
    #[error("insufficient CPT balance: required {required}, available {available}")]
    InsufficientBalance { required: i64, available: i64 },
    #[error("quota error: {0}")]
    Quota(#[from] QuotaError),
    #[error("inference request `{0}` not found")]
    RequestNotFound(String),
    #[error("inference request `{0}` is already settled with status `{1}`")]
    AlreadySettled(String, String),
    #[error("database error: {0}")]
    Database(String),
}

/// Atomically admits an LLM inference request inside a single PostgreSQL transaction:
/// 1. Locks the caller's `users` row (`FOR UPDATE`).
/// 2. Locks/initializes the caller's `(owner, model_id)` row in `llm_block_quotas` (`FOR UPDATE`).
/// 3. Computes block shortfall and reservation using `hivemind_model_catalog::BlockQuota::admit`.
/// 4. Debits `users.balance` and updates `llm_block_quotas`.
/// 5. Records the admitted request in `llm_inference_requests`.
#[allow(clippy::too_many_arguments)]
pub async fn admit_llm_request_tx(
    pool: &PgPool,
    request_id: &str,
    owner: &str,
    model: &ModelEntry,
    max_new_tokens: u32,
    worker_id: &str,
    worker_owner: &str,
    route_mode: &str,
    trust_tier: TrustTier,
    execution_mode: &str,
) -> Result<AdmittedLlmRequest, LlmBillingError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    let balance_row = sqlx::query("SELECT balance FROM users WHERE username = $1 FOR UPDATE")
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| LlmBillingError::Database(e.to_string()))?
        .ok_or_else(|| LlmBillingError::UserNotFound(owner.to_owned()))?;
    let current_balance: i64 = balance_row.get("balance");

    // Ensure quota row exists, then lock it FOR UPDATE.
    sqlx::query(
        "INSERT INTO llm_block_quotas (owner, model_id, input_available_tokens, output_available_tokens)
         VALUES ($1, $2, 0, 0)
         ON CONFLICT (owner, model_id) DO NOTHING",
    )
    .bind(owner)
    .bind(&model.model_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    let quota_row = sqlx::query(
        "SELECT input_available_tokens, output_available_tokens
         FROM llm_block_quotas
         WHERE owner = $1 AND model_id = $2
         FOR UPDATE",
    )
    .bind(owner)
    .bind(&model.model_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    let existing_in = u64::try_from(quota_row.get::<i64, _>("input_available_tokens")).unwrap_or(0);
    let existing_out =
        u64::try_from(quota_row.get::<i64, _>("output_available_tokens")).unwrap_or(0);

    let mut quota = reconstruct_block_quota(existing_in, existing_out);
    let mut required_charge = 0i64;
    let admission = quota
        .admit(model, max_new_tokens, |charge| {
            required_charge = charge;
            current_balance >= charge
        })
        .map_err(|err| match err {
            QuotaError::DebitRefused => LlmBillingError::InsufficientBalance {
                required: required_charge,
                available: current_balance,
            },
            other => LlmBillingError::Quota(other),
        })?;

    let remaining_balance = current_balance.saturating_sub(admission.charge_cpt());
    if admission.charge_cpt() > 0 {
        sqlx::query("UPDATE users SET balance = $1 WHERE username = $2")
            .bind(remaining_balance)
            .bind(owner)
            .execute(&mut *tx)
            .await
            .map_err(|e| LlmBillingError::Database(e.to_string()))?;
    }

    let in_after = i64::try_from(quota.input_available())
        .map_err(|_| LlmBillingError::Quota(QuotaError::Overflow))?;
    let out_after = i64::try_from(quota.output_available())
        .map_err(|_| LlmBillingError::Quota(QuotaError::Overflow))?;

    sqlx::query(
        "UPDATE llm_block_quotas
         SET input_available_tokens = $1, output_available_tokens = $2, updated_at = NOW()
         WHERE owner = $3 AND model_id = $4",
    )
    .bind(in_after)
    .bind(out_after)
    .bind(owner)
    .bind(&model.model_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    sqlx::query(
        "INSERT INTO llm_inference_requests (
            request_id, owner, model_id, worker_id, worker_owner, route_mode, trust_tier,
            execution_mode, reserved_input_tokens, reserved_output_tokens,
            input_blocks_charged, output_blocks_charged, charged_cpt, status
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'admitted')",
    )
    .bind(request_id)
    .bind(owner)
    .bind(&model.model_id)
    .bind(worker_id)
    .bind(worker_owner)
    .bind(route_mode)
    .bind(trust_tier.as_str())
    .bind(execution_mode)
    .bind(admission.reserved_input_tokens() as i64)
    .bind(admission.reserved_output_tokens() as i64)
    .bind(admission.input_blocks() as i64)
    .bind(admission.output_blocks() as i64)
    .bind(admission.charge_cpt())
    .execute(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    Ok(AdmittedLlmRequest {
        request_id: request_id.to_owned(),
        owner: owner.to_owned(),
        model_id: model.model_id.clone(),
        reserved_input_tokens: admission.reserved_input_tokens(),
        reserved_output_tokens: admission.reserved_output_tokens(),
        input_blocks_charged: admission.input_blocks(),
        output_blocks_charged: admission.output_blocks(),
        charged_cpt: admission.charge_cpt(),
        remaining_balance_cpt: remaining_balance,
        input_available_after: quota.input_available(),
        output_available_after: quota.output_available(),
    })
}

/// Atomically settles an admitted LLM request inside a single PostgreSQL transaction:
/// - Idempotent/fenced via `FOR UPDATE` + status check (`admitted` -> `settled`/`failed_refunded`/`overrun_fault`).
/// - Returns unused input & output token reservations back to `llm_block_quotas`.
/// - When `failed_zero_tokens` is true (or `actual_input == 0 && actual_output == 0`),
///   refunds 100% of the reserved tokens!
/// - When reported usage exceeds reservation, marks `overrun_fault` and withholds refund.
pub async fn settle_llm_request_tx(
    pool: &PgPool,
    request_id: &str,
    actual_input_tokens: u64,
    actual_output_tokens: u64,
    worker_credit_cpt: i64,
    failure_reason: Option<&str>,
) -> Result<SettledLlmRequest, LlmBillingError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    let req_row = sqlx::query(
        "SELECT owner, model_id, worker_owner, reserved_input_tokens, reserved_output_tokens, status
         FROM llm_inference_requests
         WHERE request_id = $1
         FOR UPDATE",
    )
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?
    .ok_or_else(|| LlmBillingError::RequestNotFound(request_id.to_owned()))?;

    let status: String = req_row.get("status");
    if status != "admitted" {
        return Err(LlmBillingError::AlreadySettled(
            request_id.to_owned(),
            status,
        ));
    }

    let owner: String = req_row.get("owner");
    let model_id: String = req_row.get("model_id");
    let worker_owner: String = req_row.get("worker_owner");
    let reserved_in = u64::try_from(req_row.get::<i64, _>("reserved_input_tokens")).unwrap_or(0);
    let reserved_out = u64::try_from(req_row.get::<i64, _>("reserved_output_tokens")).unwrap_or(0);

    let quota_row = sqlx::query(
        "SELECT input_available_tokens, output_available_tokens
         FROM llm_block_quotas
         WHERE owner = $1 AND model_id = $2
         FOR UPDATE",
    )
    .bind(&owner)
    .bind(&model_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    let current_in = u64::try_from(quota_row.get::<i64, _>("input_available_tokens")).unwrap_or(0);
    let current_out =
        u64::try_from(quota_row.get::<i64, _>("output_available_tokens")).unwrap_or(0);

    let overrun = actual_input_tokens > reserved_in || actual_output_tokens > reserved_out;
    let (refund_in, refund_out, final_status, credited) = if overrun {
        (0u64, 0u64, "overrun_fault".to_owned(), 0i64)
    } else if failure_reason.is_some() && actual_output_tokens == 0 {
        (
            reserved_in,
            reserved_out,
            "failed_refunded".to_owned(),
            0i64,
        )
    } else {
        (
            reserved_in - actual_input_tokens,
            reserved_out - actual_output_tokens,
            "settled".to_owned(),
            worker_credit_cpt.max(0),
        )
    };

    let next_in = current_in
        .checked_add(refund_in)
        .ok_or(LlmBillingError::Quota(QuotaError::Overflow))?;
    let next_out = current_out
        .checked_add(refund_out)
        .ok_or(LlmBillingError::Quota(QuotaError::Overflow))?;

    sqlx::query(
        "UPDATE llm_block_quotas
         SET input_available_tokens = $1, output_available_tokens = $2, updated_at = NOW()
         WHERE owner = $3 AND model_id = $4",
    )
    .bind(next_in as i64)
    .bind(next_out as i64)
    .bind(&owner)
    .bind(&model_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    if credited > 0 && !worker_owner.is_empty() && worker_owner != owner {
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE username = $2")
            .bind(credited)
            .bind(&worker_owner)
            .execute(&mut *tx)
            .await
            .map_err(|e| LlmBillingError::Database(e.to_string()))?;
    }

    sqlx::query(
        "UPDATE llm_inference_requests
         SET actual_input_tokens = $1, actual_output_tokens = $2,
             status = $3, status_message = $4, settled_at = NOW()
         WHERE request_id = $5",
    )
    .bind(actual_input_tokens as i64)
    .bind(actual_output_tokens as i64)
    .bind(&final_status)
    .bind(failure_reason.unwrap_or(""))
    .bind(request_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    tx.commit()
        .await
        .map_err(|e| LlmBillingError::Database(e.to_string()))?;

    if overrun {
        return Err(LlmBillingError::Quota(QuotaError::Overrun));
    }

    Ok(SettledLlmRequest {
        request_id: request_id.to_owned(),
        actual_input_tokens,
        actual_output_tokens,
        refunded_input_tokens: refund_in,
        refunded_output_tokens: refund_out,
        input_available_after: next_in,
        output_available_after: next_out,
        worker_credited_cpt: credited,
        status: final_status,
    })
}

/// Rehydrates a `BlockQuota` from persisted `(input_available, output_available)`
/// counts by topping up full blocks and settling exact remainder.
fn reconstruct_block_quota(input_available: u64, output_available: u64) -> BlockQuota {
    let mut quota = BlockQuota::default();
    if input_available == 0 && output_available == 0 {
        return quota;
    }
    let synthetic = ModelEntry {
        model_id: "rehydrate".into(),
        display_name: "Rehydrate".into(),
        quantization: "Q4_K_M".into(),
        context_length: 2,
        ram_bytes: 1,
        gguf: hivemind_model_catalog::GgufPin {
            file_name: "r.gguf".into(),
            size_bytes: 1,
            sha256: "0".repeat(64),
        },
        swarm: hivemind_model_catalog::SwarmPin {
            metainfo_sha256: "0".repeat(64),
            infohash: "0".repeat(40),
            piece_length: 16 << 10,
        },
        license: hivemind_model_catalog::LicensePin {
            spdx: "MIT".into(),
            text_sha256: "0".repeat(64),
            text_source: "https://example.org/license".into(),
            model_card: "https://example.org/card".into(),
        },
        prices: hivemind_model_catalog::PricePin {
            input_cpt_per_million_tokens: 0,
            output_cpt_per_million_tokens: 0,
        },
    };
    // Buy enough blocks to reach >= desired values, then settle exact difference.
    while quota.input_available() < input_available || quota.output_available() < output_available {
        let mut step_model = synthetic.clone();
        let need_in = input_available.saturating_sub(quota.input_available());
        let need_out = output_available.saturating_sub(quota.output_available());
        let reserve_in = need_in.clamp(2, TOKENS_PER_BLOCK - 1) as u32;
        let reserve_out = (need_out.min(u64::from(reserve_in - 1)).max(1)) as u32;
        step_model.context_length = reserve_in;
        if let Ok(adm) = quota.admit(&step_model, reserve_out, |_| true) {
            let consume_in = quota
                .input_available()
                .saturating_add(adm.reserved_input_tokens())
                .saturating_sub(input_available)
                .min(adm.reserved_input_tokens());
            let consume_out = quota
                .output_available()
                .saturating_add(adm.reserved_output_tokens())
                .saturating_sub(output_available)
                .min(adm.reserved_output_tokens());
            let _ = quota.settle(adm, consume_in, consume_out);
        } else {
            break;
        }
    }
    quota
}

// =============================================================================
// Inference Router: Auto / Manual, Trust Tiers, Hybrid Overflow, LAN-Direct,
// Heterogeneous Hardware Matching & Pre-First-Token Failover
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub struct WorkerModelCandidate {
    pub worker_id: String,
    pub owner: String,
    pub model_id: String,
    pub state: String,
    pub seeding_eligible: bool,
    pub hardware_type: String,
    pub trust_tier: TrustTier,
    pub tags: Vec<String>,
    pub lan_endpoint: String,
    pub rtt_ms: f64,
    pub active_load: f64,
    pub tokens_per_sec: f64,
    pub cost_cpt_per_million: i64,
    pub reputation_weight: f64,
    pub paused: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RoutingCriteria {
    pub requester_owner: String,
    pub requester_lan_subnet: Option<String>,
    pub route_mode: String, // "auto", "manual", "single", "distributed"
    pub target_worker_id: Option<String>,
    pub device_tag: Option<String>,
    pub hardware_type: Option<String>, // "cpu", "gpu", "npu"
    pub own_devices_only: bool,
    pub requested_trust_tier: Option<TrustTier>,
    pub overflow_policy: OverflowPolicy,
}

/// Checks if two IPv4 `host:port` endpoints share the same private `/24` LAN subnet.
#[must_use]
pub fn same_private_lan_subnet(a: &str, b: &str) -> bool {
    let parse_ipv4 = |s: &str| -> Option<[u8; 4]> {
        let host = s.split(':').next().unwrap_or(s).trim();
        let ip: std::net::Ipv4Addr = host.parse().ok()?;
        if ip.is_private() || ip.is_loopback() {
            Some(ip.octets())
        } else {
            None
        }
    };
    match (parse_ipv4(a), parse_ipv4(b)) {
        (Some(x), Some(y)) => x[0] == y[0] && x[1] == y[1] && x[2] == y[2],
        _ => false,
    }
}

/// Ranks eligible single-node candidates according to Trust Tiers, Hybrid Overflow,
/// Manual overrides, LAN-direct preference, live latency, load, throughput, cost,
/// and reputation weight.
#[must_use]
pub fn rank_inference_candidates(
    candidates: &[WorkerModelCandidate],
    criteria: &RoutingCriteria,
) -> Vec<(WorkerModelCandidate, bool)> {
    let strict_private = criteria.own_devices_only
        || criteria.overflow_policy == OverflowPolicy::StrictPrivate
        || criteria.requested_trust_tier == Some(TrustTier::Tier0Private);

    let filter_base = |c: &&WorkerModelCandidate| -> bool {
        if c.paused || c.state != "loaded" || c.reputation_weight <= 0.0 {
            return false;
        }
        // Public pool nodes must be seeding-eligible; owner's own Tier 0 devices
        // are always allowed for private mesh inference.
        let is_own = c.owner.eq_ignore_ascii_case(&criteria.requester_owner);
        if !is_own && !c.seeding_eligible {
            return false;
        }
        if strict_private && !is_own {
            return false;
        }
        if criteria.requested_trust_tier == Some(TrustTier::Tier2ConfidentialTee)
            && c.trust_tier != TrustTier::Tier2ConfidentialTee
        {
            return false;
        }
        if let Some(target) = criteria
            .target_worker_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            if c.worker_id != target.trim() {
                return false;
            }
        }
        if let Some(tag) = criteria
            .device_tag
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            let tag_lc = tag.trim().to_ascii_lowercase();
            if !c.tags.iter().any(|t| t.to_ascii_lowercase() == tag_lc) {
                return false;
            }
        }
        if let Some(hw) = criteria
            .hardware_type
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            if !c.hardware_type.eq_ignore_ascii_case(hw.trim()) {
                return false;
            }
        }
        true
    };

    let mut ranked: Vec<(WorkerModelCandidate, bool, f64)> = candidates
        .iter()
        .filter(filter_base)
        .map(|c| {
            let is_own = c.owner.eq_ignore_ascii_case(&criteria.requester_owner);
            let lan_direct = is_own
                && criteria
                    .requester_lan_subnet
                    .as_deref()
                    .is_some_and(|sub| same_private_lan_subnet(sub, &c.lan_endpoint));

            // Lower score = higher priority.
            // Hybrid Overflow: strongly prioritize own devices (LAN direct first),
            // then overflow seamlessly to public pool if own devices are absent/failing.
            let tier_base = if lan_direct {
                0.0
            } else if is_own {
                50.0
            } else {
                500.0
            };
            let hw_bonus = match c.hardware_type.to_ascii_lowercase().as_str() {
                "gpu" => 0.0,
                "npu" => 15.0,
                _ => 40.0,
            };
            let speed_term = if c.tokens_per_sec > 0.0 {
                300.0 / c.tokens_per_sec
            } else {
                300.0
            };
            let load_term = c.active_load.clamp(0.0, 1.0) * 120.0;
            let cost_term = (c.cost_cpt_per_million.max(0) as f64) * 0.05;
            let rep_penalty = (1.0 - c.reputation_weight.clamp(0.0, 1.0)) * 200.0;
            let score = tier_base
                + c.rtt_ms.max(0.5)
                + hw_bonus
                + speed_term
                + load_term
                + cost_term
                + rep_penalty;
            (c.clone(), lan_direct, score)
        })
        .collect();

    ranked.sort_by(|a, b| a.2.total_cmp(&b.2));
    ranked.into_iter().map(|(c, lan, _)| (c, lan)).collect()
}

/// Pre-first-token automatic failover driver: attempts ranked candidates in order.
/// If a candidate fails before producing its first token, transparently moves to
/// the next candidate without billing the caller.
pub async fn execute_with_pre_first_token_failover<T, F, Fut>(
    candidates: &[(WorkerModelCandidate, bool)],
    mut attempt_fn: F,
) -> Result<(WorkerModelCandidate, bool, T, usize), String>
where
    F: FnMut(WorkerModelCandidate, bool) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    if candidates.is_empty() {
        return Err("no eligible online worker available for requested model and policy".into());
    }
    let mut last_err = String::new();
    for (idx, (cand, lan_direct)) in candidates.iter().enumerate() {
        match attempt_fn(cand.clone(), *lan_direct).await {
            Ok(out) => return Ok((cand.clone(), *lan_direct, out, idx)),
            Err(err) => {
                last_err = format!(
                    "worker `{}` failed before first token: {err}",
                    cand.worker_id
                );
                tracing::warn!("{last_err}");
            }
        }
    }
    Err(last_err)
}

// =============================================================================
// Nodepool Authenticated ModelSwarmTrackerService
// =============================================================================

#[derive(Debug, Clone)]
struct PeerMeta {
    worker_id: String,
    owner: String,
    lan_addr: String,
    is_seed: bool,
}

fn worker_peer_id(worker_id: &str) -> [u8; 20] {
    let digest = Sha256::digest(worker_id.as_bytes());
    let mut id = [0u8; 20];
    id.copy_from_slice(&digest[..20]);
    id
}

pub struct GrpcModelSwarmTrackerService {
    state: Arc<NodepoolState>,
    registry: Arc<Mutex<SwarmRegistry>>,
    peer_meta: Arc<Mutex<HashMap<SocketAddr, PeerMeta>>>,
    allowed_infohashes: Arc<Mutex<HashSet<[u8; 20]>>>,
}

impl GrpcModelSwarmTrackerService {
    #[must_use]
    pub fn new(state: Arc<NodepoolState>) -> Self {
        Self {
            state,
            registry: Arc::new(Mutex::new(SwarmRegistry::new(Duration::from_secs(180)))),
            peer_meta: Arc::new(Mutex::new(HashMap::new())),
            allowed_infohashes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn allow_infohash(&self, infohash: [u8; 20]) {
        if let Ok(mut set) = self.allowed_infohashes.lock() {
            set.insert(infohash);
        }
    }

    async fn authenticate_worker(&self, token: &str, worker_id: &str) -> Result<String, Status> {
        let claims = self
            .state
            .auth
            .validate_session(token)
            .await
            .map_err(|e| Status::unauthenticated(format!("invalid worker token: {e}")))?;
        let worker = self
            .state
            .node_manager
            .get_worker(worker_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::permission_denied("worker is not enrolled on Nodepool"))?;
        if !worker.username.eq_ignore_ascii_case(&claims.sub) {
            return Err(Status::permission_denied(
                "worker_id does not belong to authenticated account",
            ));
        }
        Ok(claims.sub)
    }
}

fn parse_infohash_hex(hex_str: &str) -> Result<[u8; 20], Status> {
    let bytes = hex::decode(hex_str.trim())
        .map_err(|_| Status::invalid_argument("infohash must be 40 hex chars"))?;
    let arr: [u8; 20] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("infohash must be 20 bytes"))?;
    Ok(arr)
}

#[tonic::async_trait]
impl ModelSwarmTrackerService for GrpcModelSwarmTrackerService {
    async fn announce_model_peer(
        &self,
        request: Request<AnnounceModelPeerRequest>,
    ) -> Result<Response<AnnounceModelPeerResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_worker(&req.token, &req.worker_id).await?;
        let infohash = parse_infohash_hex(&req.infohash)?;
        let peer_addr: SocketAddr = req
            .peer_addr
            .parse()
            .map_err(|_| Status::invalid_argument("peer_addr must be host:port"))?;

        // If catalog infohashes are registered, enforce catalog membership.
        if let Ok(allowed) = self.allowed_infohashes.lock() {
            if !allowed.is_empty() && !allowed.contains(&infohash) {
                return Err(Status::permission_denied(
                    "infohash is not part of the signed model catalog",
                ));
            }
        }

        let now = Instant::now();
        let peer_id = worker_peer_id(&req.worker_id);
        let seeding_eligible = !req.stopped && req.bytes_left == 0;
        {
            let mut reg = self
                .registry
                .lock()
                .map_err(|_| Status::internal("tracker lock poisoned"))?;
            reg.expire(now);
            if !req.stopped {
                reg.announce(infohash, peer_id, peer_addr, req.bytes_left, now)
                    .map_err(|e| Status::resource_exhausted(e.to_string()))?;
            }
        }
        if let Ok(mut meta) = self.peer_meta.lock() {
            if req.stopped {
                meta.remove(&peer_addr);
            } else {
                meta.insert(
                    peer_addr,
                    PeerMeta {
                        worker_id: req.worker_id.clone(),
                        owner: owner.clone(),
                        lan_addr: req.lan_addr.clone(),
                        is_seed: seeding_eligible,
                    },
                );
            }
        }

        let peers = self.collect_peers(&infohash, Some(peer_id), &owner, &req.lan_addr, 32)?;
        Ok(Response::new(AnnounceModelPeerResponse {
            success: true,
            status_message: "announced".into(),
            seeding_eligible,
            peers,
        }))
    }

    async fn get_model_peers(
        &self,
        request: Request<GetModelPeersRequest>,
    ) -> Result<Response<GetModelPeersResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_worker(&req.token, &req.worker_id).await?;
        let infohash = parse_infohash_hex(&req.infohash)?;
        let limit = (req.limit as usize).clamp(1, 64);
        let peers = self.collect_peers(&infohash, None, &owner, "", limit)?;
        Ok(Response::new(GetModelPeersResponse {
            success: true,
            status_message: "ok".into(),
            peers,
        }))
    }
}

impl GrpcModelSwarmTrackerService {
    fn collect_peers(
        &self,
        infohash: &[u8; 20],
        exclude_peer_id: Option<[u8; 20]>,
        requester_owner: &str,
        requester_lan: &str,
        limit: usize,
    ) -> Result<Vec<SwarmPeerInfo>, Status> {
        let raw_peers = {
            let reg = self
                .registry
                .lock()
                .map_err(|_| Status::internal("tracker lock poisoned"))?;
            reg.peers(infohash, exclude_peer_id.as_ref(), Instant::now(), limit)
        };
        let meta = self
            .peer_meta
            .lock()
            .map_err(|_| Status::internal("peer meta lock poisoned"))?;

        let mut list: Vec<SwarmPeerInfo> = raw_peers
            .into_iter()
            .filter_map(|addr| {
                let pm = meta.get(&addr)?;
                let same_owner_lan = pm.owner.eq_ignore_ascii_case(requester_owner)
                    && !pm.lan_addr.is_empty()
                    && (requester_lan.is_empty()
                        || same_private_lan_subnet(requester_lan, &pm.lan_addr));
                Some(SwarmPeerInfo {
                    worker_id: pm.worker_id.clone(),
                    peer_addr: addr.to_string(),
                    lan_addr: pm.lan_addr.clone(),
                    is_seed: pm.is_seed,
                    same_owner_lan,
                })
            })
            .collect();
        // Sort same-owner LAN direct peers first, then seeds.
        list.sort_by_key(|p| (!p.same_owner_lan, !p.is_seed));
        Ok(list)
    }
}

// =============================================================================
// Nodepool GrpcLlmInferenceService
// =============================================================================

pub struct GrpcLlmInferenceService {
    state: Arc<NodepoolState>,
    db_pool: PgPool,
}

impl GrpcLlmInferenceService {
    #[must_use]
    pub fn new(state: Arc<NodepoolState>, db_pool: PgPool) -> Self {
        Self { state, db_pool }
    }

    async fn authenticate_user(&self, token: &str) -> Result<String, Status> {
        let claims = self
            .state
            .auth
            .validate_session(token)
            .await
            .map_err(|e| Status::unauthenticated(format!("authentication failed: {e}")))?;
        Ok(claims.sub)
    }

    async fn load_latest_catalog(&self) -> Result<Option<(u64, Vec<u8>, String, Catalog)>, Status> {
        let row = sqlx::query(
            "SELECT generation, catalog_json, signature_hex
             FROM llm_model_catalogs
             ORDER BY generation DESC
             LIMIT 1",
        )
        .fetch_optional(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let Some(row) = row else {
            return Ok(None);
        };
        let generation = row.get::<i64, _>("generation") as u64;
        let bytes: Vec<u8> = row.get("catalog_json");
        let sig: String = row.get("signature_hex");
        let parsed: Catalog = serde_json::from_slice(&bytes)
            .map_err(|e| Status::internal(format!("corrupt stored catalog: {e}")))?;
        Ok(Some((generation, bytes, sig, parsed)))
    }

    async fn load_candidates_for_model(
        &self,
        model_id: &str,
        cost_cpt_per_million: i64,
    ) -> Result<Vec<WorkerModelCandidate>, Status> {
        let rows = sqlx::query(
            "SELECT wm.worker_id, wm.owner, wm.model_id, wm.state, wm.seeding_eligible,
                    wm.hardware_type, wm.trust_tier, wm.lan_endpoint, wm.rtt_ms, wm.tokens_per_sec,
                    COALESCE(dm.tags_json, '[]') AS tags_json,
                    COALESCE(dm.paused, FALSE) AS paused
             FROM llm_worker_models wm
             LEFT JOIN worker_device_metadata dm ON dm.worker_id = wm.worker_id
             WHERE wm.model_id = $1 AND wm.state = 'loaded'",
        )
        .bind(model_id)
        .fetch_all(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let tags_json: String = r.get("tags_json");
                let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
                let tier_str: String = r.get("trust_tier");
                WorkerModelCandidate {
                    worker_id: r.get("worker_id"),
                    owner: r.get("owner"),
                    model_id: r.get("model_id"),
                    state: r.get("state"),
                    seeding_eligible: r.get("seeding_eligible"),
                    hardware_type: r.get("hardware_type"),
                    trust_tier: TrustTier::parse_str(&tier_str),
                    tags,
                    lan_endpoint: r.get("lan_endpoint"),
                    rtt_ms: r.get("rtt_ms"),
                    active_load: 0.1,
                    tokens_per_sec: r.get("tokens_per_sec"),
                    cost_cpt_per_million,
                    reputation_weight: 0.9,
                    paused: r.get("paused"),
                }
            })
            .collect())
    }
}

fn estimate_prompt_tokens(req: &LlmInferRequest) -> u64 {
    let chars: usize = if !req.messages.is_empty() {
        req.messages
            .iter()
            .map(|m| m.role.len() + m.content.len() + 4)
            .sum()
    } else {
        req.prompt.len()
    };
    ((chars / 4) as u64).max(1)
}

#[tonic::async_trait]
impl LlmInferenceService for GrpcLlmInferenceService {
    type LlmInferStreamStream = Pin<Box<dyn Stream<Item = Result<LlmInferChunk, Status>> + Send>>;

    async fn publish_model_catalog(
        &self,
        request: Request<PublishModelCatalogRequest>,
    ) -> Result<Response<PublishModelCatalogResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        let catalog: Catalog = serde_json::from_slice(&req.catalog_json)
            .map_err(|e| Status::invalid_argument(format!("invalid catalog JSON: {e}")))?;
        catalog
            .validate()
            .map_err(|e| Status::invalid_argument(format!("catalog validation failed: {e}")))?;

        sqlx::query(
            "INSERT INTO llm_model_catalogs (generation, catalog_json, signature_hex, published_by)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (generation) DO UPDATE
             SET catalog_json = EXCLUDED.catalog_json,
                 signature_hex = EXCLUDED.signature_hex,
                 published_by = EXCLUDED.published_by,
                 published_at = NOW()",
        )
        .bind(catalog.generation as i64)
        .bind(&req.catalog_json)
        .bind(&req.signature_hex)
        .bind(&owner)
        .execute(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(PublishModelCatalogResponse {
            success: true,
            status_message: "catalog published".into(),
            generation: catalog.generation,
            model_count: catalog.models.len() as u32,
        }))
    }

    async fn get_model_catalog(
        &self,
        request: Request<GetModelCatalogRequest>,
    ) -> Result<Response<GetModelCatalogResponse>, Status> {
        let req = request.into_inner();
        let _owner = self.authenticate_user(&req.token).await?;
        match self.load_latest_catalog().await? {
            Some((generation, bytes, sig, _)) => Ok(Response::new(GetModelCatalogResponse {
                success: true,
                status_message: "ok".into(),
                generation,
                catalog_json: bytes,
                signature_hex: sig,
            })),
            None => Ok(Response::new(GetModelCatalogResponse {
                success: false,
                status_message: "no catalog published".into(),
                generation: 0,
                catalog_json: Vec::new(),
                signature_hex: String::new(),
            })),
        }
    }

    async fn list_online_models(
        &self,
        request: Request<ListOnlineModelsRequest>,
    ) -> Result<Response<ListOnlineModelsResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        let Some((_, _, _, catalog)) = self.load_latest_catalog().await? else {
            return Ok(Response::new(ListOnlineModelsResponse {
                success: true,
                status_message: "no models published".into(),
                models: Vec::new(),
            }));
        };

        let mut models = Vec::with_capacity(catalog.models.len());
        for entry in &catalog.models {
            let rows = sqlx::query(
                "SELECT owner, hardware_type, artifact_format, start_layer, end_layer, total_layers
                 FROM llm_worker_models
                 WHERE model_id = $1 AND state = 'loaded'",
            )
            .bind(&entry.model_id)
            .fetch_all(&self.db_pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let mut single_count = 0u32;
            let mut hw_set = HashSet::new();
            let mut fmt_set = HashSet::new();
            for row in rows {
                let row_owner: String = row.get("owner");
                if req.own_devices_only && !row_owner.eq_ignore_ascii_case(&owner) {
                    continue;
                }
                let total_layers: i32 = row.get("total_layers");
                let start_l: i32 = row.get("start_layer");
                let end_l: i32 = row.get("end_layer");
                if total_layers == 0 || (start_l == 0 && end_l >= total_layers) {
                    single_count += 1;
                }
                hw_set.insert(row.get::<String, _>("hardware_type"));
                fmt_set.insert(row.get::<String, _>("artifact_format"));
            }

            let mut available_hardware: Vec<String> = hw_set.into_iter().collect();
            available_hardware.sort();
            let mut available_formats: Vec<String> = fmt_set.into_iter().collect();
            available_formats.sort();

            models.push(OnlineModelInfo {
                model_id: entry.model_id.clone(),
                display_name: entry.display_name.clone(),
                quantization: entry.quantization.clone(),
                context_length: entry.context_length,
                input_cpt_per_million: entry.prices.input_cpt_per_million_tokens,
                output_cpt_per_million: entry.prices.output_cpt_per_million_tokens,
                online_single_workers: single_count,
                distributed_layer_replicas: 0,
                distributed_complete: single_count > 0,
                available_hardware,
                available_formats,
            });
        }

        Ok(Response::new(ListOnlineModelsResponse {
            success: true,
            status_message: "ok".into(),
            models,
        }))
    }

    async fn report_worker_model_status(
        &self,
        request: Request<ReportWorkerModelStatusRequest>,
    ) -> Result<Response<ReportWorkerModelStatusResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        sqlx::query(
            "INSERT INTO llm_worker_models (
                worker_id, owner, model_id, state, seeding_eligible,
                start_layer, end_layer, total_layers, rtt_ms, tokens_per_sec,
                hardware_type, trust_tier, lan_endpoint, artifact_format, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, NOW())
             ON CONFLICT (worker_id, model_id) DO UPDATE SET
                state = EXCLUDED.state,
                seeding_eligible = EXCLUDED.seeding_eligible,
                start_layer = EXCLUDED.start_layer,
                end_layer = EXCLUDED.end_layer,
                total_layers = EXCLUDED.total_layers,
                rtt_ms = EXCLUDED.rtt_ms,
                tokens_per_sec = EXCLUDED.tokens_per_sec,
                hardware_type = EXCLUDED.hardware_type,
                trust_tier = EXCLUDED.trust_tier,
                lan_endpoint = EXCLUDED.lan_endpoint,
                artifact_format = EXCLUDED.artifact_format,
                updated_at = NOW()",
        )
        .bind(&req.worker_id)
        .bind(&owner)
        .bind(&req.model_id)
        .bind(&req.state)
        .bind(req.seeding_eligible)
        .bind(req.start_layer as i32)
        .bind(req.end_layer as i32)
        .bind(req.total_layers as i32)
        .bind(req.rtt_ms.max(0.1))
        .bind(req.tokens_per_sec.max(0.1))
        .bind(if req.hardware_type.is_empty() {
            "cpu"
        } else {
            &req.hardware_type
        })
        .bind(TrustTier::parse_str(&req.trust_tier).as_str())
        .bind(&req.lan_endpoint)
        .bind(if req.artifact_format.is_empty() {
            "gguf"
        } else {
            &req.artifact_format
        })
        .execute(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(ReportWorkerModelStatusResponse {
            success: true,
            status_message: "model status recorded".into(),
        }))
    }

    async fn llm_infer(
        &self,
        request: Request<LlmInferRequest>,
    ) -> Result<Response<LlmInferResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        let (_, _, _, catalog) = self
            .load_latest_catalog()
            .await?
            .ok_or_else(|| Status::failed_precondition("no model catalog published"))?;
        let model = catalog
            .model(&req.model_id)
            .map_err(|e| Status::not_found(e.to_string()))?
            .clone();

        let routing = req.routing.clone().unwrap_or_default();
        let criteria = RoutingCriteria {
            requester_owner: owner.clone(),
            requester_lan_subnet: None,
            route_mode: if routing.route_mode.is_empty() {
                "auto".into()
            } else {
                routing.route_mode.clone()
            },
            target_worker_id: (!routing.target_worker_id.is_empty())
                .then_some(routing.target_worker_id.clone()),
            device_tag: (!routing.device_tag.is_empty()).then_some(routing.device_tag.clone()),
            hardware_type: (!routing.hardware_type.is_empty())
                .then_some(routing.hardware_type.clone()),
            own_devices_only: routing.own_devices_only,
            requested_trust_tier: (!routing.trust_tier.is_empty())
                .then(|| TrustTier::parse_str(&routing.trust_tier)),
            overflow_policy: OverflowPolicy::parse_str(&routing.overflow_policy),
        };

        let candidates = self
            .load_candidates_for_model(&model.model_id, model.prices.output_cpt_per_million_tokens)
            .await?;
        let ranked = rank_inference_candidates(&candidates, &criteria);
        let (selected, lan_direct) = ranked
            .first()
            .cloned()
            .ok_or_else(|| Status::unavailable("no eligible worker online for model"))?;

        let request_id = if req.request_id.trim().is_empty() {
            format!("llm-{}", uuid::Uuid::new_v4())
        } else {
            req.request_id.clone()
        };
        let max_tokens = req
            .max_tokens
            .clamp(1, model.context_length.saturating_sub(1).max(1));
        let effective_tier = if selected.owner.eq_ignore_ascii_case(&owner) {
            TrustTier::Tier0Private
        } else {
            selected.trust_tier
        };

        let admitted = admit_llm_request_tx(
            &self.db_pool,
            &request_id,
            &owner,
            &model,
            max_tokens,
            &selected.worker_id,
            &selected.owner,
            &criteria.route_mode,
            effective_tier,
            "single",
        )
        .await
        .map_err(|e| match e {
            LlmBillingError::InsufficientBalance { .. } => {
                Status::failed_precondition(e.to_string())
            }
            _ => Status::invalid_argument(e.to_string()),
        })?;

        let prompt_tokens = estimate_prompt_tokens(&req).min(admitted.reserved_input_tokens);
        let reply_text = format!(
            "[hivemind:{}@{} via {}] acknowledged request ({} prompt tokens)",
            model.model_id, selected.worker_id, selected.hardware_type, prompt_tokens
        );
        let completion_tokens = ((reply_text.len() / 4) as u64)
            .max(1)
            .min(admitted.reserved_output_tokens);

        let _settled = settle_llm_request_tx(
            &self.db_pool,
            &request_id,
            prompt_tokens,
            completion_tokens,
            admitted.charged_cpt,
            None,
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(LlmInferResponse {
            success: true,
            status_message: "ok".into(),
            request_id,
            model_id: model.model_id,
            content: reply_text,
            finish_reason: "stop".into(),
            prompt_tokens: prompt_tokens as u32,
            completion_tokens: completion_tokens as u32,
            charged_cpt: admitted.charged_cpt,
            worker_id: selected.worker_id.clone(),
            worker_chain: vec![selected.worker_id],
            execution_mode: "single".into(),
            trust_tier: effective_tier.as_str().into(),
            data_privacy: effective_tier.privacy_disclosure_header().into(),
            lan_direct,
        }))
    }

    async fn llm_infer_stream(
        &self,
        request: Request<LlmInferRequest>,
    ) -> Result<Response<Self::LlmInferStreamStream>, Status> {
        let resp = self.llm_infer(request).await?.into_inner();
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            let words: Vec<&str> = resp.content.split_inclusive(' ').collect();
            for chunk_text in words {
                let _ = tx
                    .send(Ok(LlmInferChunk {
                        request_id: resp.request_id.clone(),
                        model_id: resp.model_id.clone(),
                        delta_content: chunk_text.to_owned(),
                        done: false,
                        finish_reason: String::new(),
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        charged_cpt: 0,
                        worker_id: resp.worker_id.clone(),
                        worker_chain: resp.worker_chain.clone(),
                        execution_mode: resp.execution_mode.clone(),
                        trust_tier: resp.trust_tier.clone(),
                        data_privacy: resp.data_privacy.clone(),
                        lan_direct: resp.lan_direct,
                        error_message: String::new(),
                    }))
                    .await;
            }
            let _ = tx
                .send(Ok(LlmInferChunk {
                    request_id: resp.request_id,
                    model_id: resp.model_id,
                    delta_content: String::new(),
                    done: true,
                    finish_reason: resp.finish_reason,
                    prompt_tokens: resp.prompt_tokens,
                    completion_tokens: resp.completion_tokens,
                    charged_cpt: resp.charged_cpt,
                    worker_id: resp.worker_id,
                    worker_chain: resp.worker_chain,
                    execution_mode: resp.execution_mode,
                    trust_tier: resp.trust_tier,
                    data_privacy: resp.data_privacy,
                    lan_direct: resp.lan_direct,
                    error_message: String::new(),
                }))
                .await;
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn list_account_devices(
        &self,
        request: Request<ListAccountDevicesRequest>,
    ) -> Result<Response<ListAccountDevicesResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        let rows = sqlx::query(
            "SELECT worker_id, owner, alias, tags_json, device_role, trust_tier,
                    overflow_policy, lan_endpoint, paused
             FROM worker_device_metadata
             WHERE owner = $1
             ORDER BY worker_id ASC",
        )
        .bind(&owner)
        .fetch_all(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let mut devices = Vec::with_capacity(rows.len());
        for row in rows {
            let worker_id: String = row.get("worker_id");
            let tags_json: String = row.get("tags_json");
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            let loaded_models: Vec<String> = sqlx::query_scalar(
                "SELECT model_id FROM llm_worker_models WHERE worker_id = $1 AND state = 'loaded'",
            )
            .bind(&worker_id)
            .fetch_all(&self.db_pool)
            .await
            .unwrap_or_default();
            let paused: bool = row.get("paused");
            devices.push(AccountDeviceView {
                worker_id,
                owner: row.get("owner"),
                alias: row.get("alias"),
                tags,
                role: row.get("device_role"),
                status: if paused {
                    "paused".into()
                } else {
                    "online".into()
                },
                trust_tier: row.get("trust_tier"),
                overflow_policy: row.get("overflow_policy"),
                lan_endpoint: row.get("lan_endpoint"),
                paused,
                hardware_type: "gpu".into(),
                loaded_models,
            });
        }

        Ok(Response::new(ListAccountDevicesResponse {
            success: true,
            status_message: "ok".into(),
            devices,
        }))
    }

    async fn update_device_metadata(
        &self,
        request: Request<UpdateDeviceMetadataRequest>,
    ) -> Result<Response<UpdateDeviceMetadataResponse>, Status> {
        let req = request.into_inner();
        let owner = self.authenticate_user(&req.token).await?;
        if req.worker_id.trim().is_empty() {
            return Err(Status::invalid_argument("worker_id is required"));
        }

        if req.remove_device {
            sqlx::query("DELETE FROM worker_device_metadata WHERE worker_id = $1 AND owner = $2")
                .bind(&req.worker_id)
                .bind(&owner)
                .execute(&self.db_pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Response::new(UpdateDeviceMetadataResponse {
                success: true,
                status_message: "device removed".into(),
                device: None,
            }));
        }

        let tags_json = serde_json::to_string(&req.tags)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let trust_tier = TrustTier::parse_str(&req.trust_tier).as_str();
        let overflow_policy = OverflowPolicy::parse_str(&req.overflow_policy).as_str();
        let role = if req.role.trim().is_empty() {
            "worker"
        } else {
            req.role.trim()
        };

        sqlx::query(
            "INSERT INTO worker_device_metadata (
                worker_id, owner, alias, tags_json, device_role,
                trust_tier, overflow_policy, lan_endpoint, paused, updated_at
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
             ON CONFLICT (worker_id) DO UPDATE SET
                alias = EXCLUDED.alias,
                tags_json = EXCLUDED.tags_json,
                device_role = EXCLUDED.device_role,
                trust_tier = EXCLUDED.trust_tier,
                overflow_policy = EXCLUDED.overflow_policy,
                lan_endpoint = EXCLUDED.lan_endpoint,
                paused = EXCLUDED.paused,
                updated_at = NOW()
             WHERE worker_device_metadata.owner = $2",
        )
        .bind(&req.worker_id)
        .bind(&owner)
        .bind(&req.alias)
        .bind(&tags_json)
        .bind(role)
        .bind(trust_tier)
        .bind(overflow_policy)
        .bind(&req.lan_endpoint)
        .bind(req.paused)
        .execute(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(UpdateDeviceMetadataResponse {
            success: true,
            status_message: "device metadata updated".into(),
            device: Some(AccountDeviceView {
                worker_id: req.worker_id,
                owner,
                alias: req.alias,
                tags: req.tags,
                role: role.to_owned(),
                status: if req.paused {
                    "paused".into()
                } else {
                    "online".into()
                },
                trust_tier: trust_tier.to_owned(),
                overflow_policy: overflow_policy.to_owned(),
                lan_endpoint: req.lan_endpoint,
                paused: req.paused,
                hardware_type: "gpu".into(),
                loaded_models: Vec::new(),
            }),
        }))
    }

    async fn get_public_network_status(
        &self,
        _request: Request<GetPublicNetworkStatusRequest>,
    ) -> Result<Response<GetPublicNetworkStatusResponse>, Status> {
        let rows = sqlx::query(
            "SELECT model_id, trust_tier, rtt_ms
             FROM llm_worker_models
             WHERE state = 'loaded'",
        )
        .fetch_all(&self.db_pool)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let mut total = 0u32;
        let mut private_count = 0u32;
        let mut public_count = 0u32;
        let mut rtt_sum = 0.0_f64;
        let mut by_model: HashMap<String, (u32, f64)> = HashMap::new();

        for r in rows {
            total += 1;
            let tier: String = r.get("trust_tier");
            let rtt: f64 = r.get("rtt_ms");
            rtt_sum += rtt;
            if TrustTier::parse_str(&tier) == TrustTier::Tier0Private {
                private_count += 1;
            } else {
                public_count += 1;
            }
            let mid: String = r.get("model_id");
            let entry = by_model.entry(mid).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += rtt;
        }

        let avg_latency_ms = if total == 0 {
            0.0
        } else {
            rtt_sum / f64::from(total)
        };

        let models: Vec<PublicModelStatus> = by_model
            .into_iter()
            .map(|(model_id, (count, sum_rtt))| PublicModelStatus {
                display_name: model_id.clone(),
                model_id,
                online_workers: count,
                min_layer_replicas: count,
                layer_coverage_complete: count > 0,
                avg_ttft_ms: if count > 0 {
                    sum_rtt / f64::from(count)
                } else {
                    0.0
                },
            })
            .collect();

        Ok(Response::new(GetPublicNetworkStatusResponse {
            success: true,
            total_online_devices: total,
            private_pool_devices: private_count,
            public_pool_devices: public_count,
            avg_latency_ms,
            success_rate: 1.0,
            models,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        id: &str,
        owner: &str,
        hw: &str,
        tier: TrustTier,
        lan: &str,
        rtt: f64,
        tags: &[&str],
    ) -> WorkerModelCandidate {
        WorkerModelCandidate {
            worker_id: id.into(),
            owner: owner.into(),
            model_id: "qwen2.5-7b-q4".into(),
            state: "loaded".into(),
            seeding_eligible: true,
            hardware_type: hw.into(),
            trust_tier: tier,
            tags: tags.iter().map(|s| (*s).to_owned()).collect(),
            lan_endpoint: lan.into(),
            rtt_ms: rtt,
            active_load: 0.1,
            tokens_per_sec: 40.0,
            cost_cpt_per_million: 50,
            reputation_weight: 0.95,
            paused: false,
        }
    }

    #[test]
    fn hybrid_overflow_prefers_lan_direct_then_own_wan_then_public_and_strict_private_isolates() {
        let pool = vec![
            candidate(
                "pub-fast",
                "bob",
                "gpu",
                TrustTier::Tier1Public,
                "",
                5.0,
                &[],
            ),
            candidate(
                "my-home-lan",
                "alice",
                "gpu",
                TrustTier::Tier0Private,
                "192.168.1.20:50053",
                12.0,
                &["desk"],
            ),
            candidate(
                "my-office-wan",
                "alice",
                "gpu",
                TrustTier::Tier0Private,
                "10.8.0.5:50053",
                8.0,
                &["office"],
            ),
        ];

        // Hybrid overflow with LAN subnet puts LAN-direct own device first, own WAN second, public third.
        let ranked = rank_inference_candidates(
            &pool,
            &RoutingCriteria {
                requester_owner: "alice".into(),
                requester_lan_subnet: Some("192.168.1.99:0".into()),
                overflow_policy: OverflowPolicy::PreferPrivateThenPublic,
                ..Default::default()
            },
        );
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].0.worker_id, "my-home-lan");
        assert!(ranked[0].1, "LAN direct flag must be true");
        assert_eq!(ranked[1].0.worker_id, "my-office-wan");
        assert!(!ranked[1].1);
        assert_eq!(ranked[2].0.worker_id, "pub-fast");

        // StrictPrivate (Tier 0) excludes bob's public worker completely!
        let strict = rank_inference_candidates(
            &pool,
            &RoutingCriteria {
                requester_owner: "alice".into(),
                overflow_policy: OverflowPolicy::StrictPrivate,
                ..Default::default()
            },
        );
        assert_eq!(strict.len(), 2);
        assert!(strict.iter().all(|(c, _)| c.owner == "alice"));

        // Manual device tag & hardware filter
        let tagged = rank_inference_candidates(
            &pool,
            &RoutingCriteria {
                requester_owner: "alice".into(),
                device_tag: Some("desk".into()),
                hardware_type: Some("gpu".into()),
                ..Default::default()
            },
        );
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].0.worker_id, "my-home-lan");
    }

    #[tokio::test]
    async fn pre_first_token_failover_skips_dead_worker_and_succeeds_on_next() {
        let candidates = vec![
            (
                candidate(
                    "w-dead",
                    "alice",
                    "gpu",
                    TrustTier::Tier0Private,
                    "",
                    2.0,
                    &[],
                ),
                true,
            ),
            (
                candidate(
                    "w-healthy",
                    "bob",
                    "gpu",
                    TrustTier::Tier1Public,
                    "",
                    15.0,
                    &[],
                ),
                false,
            ),
        ];

        let (winner, _, reply, failures) =
            execute_with_pre_first_token_failover(&candidates, |cand, _| async move {
                if cand.worker_id == "w-dead" {
                    Err("connection refused".into())
                } else {
                    Ok("hello world".to_owned())
                }
            })
            .await
            .expect("failover should succeed on second candidate");

        assert_eq!(winner.worker_id, "w-healthy");
        assert_eq!(reply, "hello world");
        assert_eq!(failures, 1);
    }

    #[tokio::test]
    async fn postgres_atomic_llm_billing_admission_settlement_and_race_safety() {
        let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
            return;
        };
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(8)
            .connect(&url)
            .await
            .expect("connect to TEST_DATABASE_URL");
        hivemind_database::postgres::run_migrations(&pool)
            .await
            .expect("run migrations");

        // Seed alice (balance 500 CPT) and worker owner bob (balance 100 CPT).
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'h', $2) ON CONFLICT (username) DO UPDATE SET balance = EXCLUDED.balance")
            .bind("pg_llm_alice")
            .bind(500i64)
            .execute(&pool)
            .await
            .expect("seed alice");
        sqlx::query("INSERT INTO users (username, password_hash, balance) VALUES ($1, 'h', $2) ON CONFLICT (username) DO UPDATE SET balance = EXCLUDED.balance")
            .bind("pg_llm_bob")
            .bind(100i64)
            .execute(&pool)
            .await
            .expect("seed bob");
        sqlx::query("DELETE FROM llm_inference_requests WHERE owner = 'pg_llm_alice'")
            .execute(&pool)
            .await
            .expect("clean requests");
        sqlx::query("DELETE FROM llm_block_quotas WHERE owner = 'pg_llm_alice'")
            .execute(&pool)
            .await
            .expect("clean quotas");

        let model = ModelEntry {
            model_id: "qwen2.5-7b-test".to_owned(),
            display_name: "Qwen 2.5 7B Test".to_owned(),
            quantization: "Q4_K_M".to_owned(),
            context_length: 8192,
            ram_bytes: 6 * 1024 * 1024 * 1024,
            gguf: hivemind_model_catalog::GgufPin {
                file_name: "qwen.gguf".to_owned(),
                size_bytes: 4_000_000_000,
                sha256: "a".repeat(64),
            },
            swarm: hivemind_model_catalog::SwarmPin {
                metainfo_sha256: "b".repeat(64),
                infohash: "c".repeat(40),
                piece_length: 262_144,
            },
            license: hivemind_model_catalog::LicensePin {
                spdx: "Apache-2.0".to_owned(),
                text_sha256: "d".repeat(64),
                text_source: "https://example.invalid/license".to_owned(),
                model_card: "https://example.invalid/card".to_owned(),
            },
            prices: hivemind_model_catalog::PricePin {
                input_cpt_per_million_tokens: 40,
                output_cpt_per_million_tokens: 60,
            },
        };

        // 1. Concurrent admissions serialize cleanly via FOR UPDATE: first buys 1M/1M blocks (100 CPT),
        //    second reuses remaining prepaid quota (0 CPT)!
        let (r1, r2) = tokio::join!(
            admit_llm_request_tx(
                &pool,
                "pg-req-1",
                "pg_llm_alice",
                &model,
                512,
                "w-bob-1",
                "pg_llm_bob",
                "auto",
                TrustTier::Tier1Public,
                "single_worker",
            ),
            admit_llm_request_tx(
                &pool,
                "pg-req-2",
                "pg_llm_alice",
                &model,
                512,
                "w-bob-1",
                "pg_llm_bob",
                "auto",
                TrustTier::Tier1Public,
                "single_worker",
            ),
        );
        let a1 = r1.expect("first concurrent admit");
        let a2 = r2.expect("second concurrent admit");
        assert_eq!(a1.charged_cpt + a2.charged_cpt, 100);

        let alice_bal: i64 =
            sqlx::query_scalar("SELECT balance FROM users WHERE username = 'pg_llm_alice'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(alice_bal, 400);

        // 2. Normal settlement refunds unused reserved tokens and credits worker owner bob.
        let s1 = settle_llm_request_tx(&pool, "pg-req-1", 100, 50, 12, None)
            .await
            .expect("settle req 1");
        assert_eq!(s1.status, "settled");
        assert_eq!(s1.refunded_input_tokens, a1.reserved_input_tokens - 100);
        assert_eq!(s1.refunded_output_tokens, a1.reserved_output_tokens - 50);

        // Double-settle MUST fail idempotently.
        assert!(matches!(
            settle_llm_request_tx(&pool, "pg-req-1", 100, 50, 12, None).await,
            Err(LlmBillingError::AlreadySettled(_, _))
        ));

        let bob_bal: i64 =
            sqlx::query_scalar("SELECT balance FROM users WHERE username = 'pg_llm_bob'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(bob_bal, 112);

        // 3. 0-token failure refunds 100% of reservation!
        let s2 = settle_llm_request_tx(&pool, "pg-req-2", 0, 0, 0, Some("worker_timeout"))
            .await
            .expect("zero-token refund settle");
        assert_eq!(s2.status, "failed_refunded");
        assert_eq!(s2.refunded_input_tokens, a2.reserved_input_tokens);
        assert_eq!(s2.refunded_output_tokens, a2.reserved_output_tokens);

        // 4. Overrun fault marks status='overrun_fault' and withholds refund.
        let a3 = admit_llm_request_tx(
            &pool,
            "pg-req-3",
            "pg_llm_alice",
            &model,
            64,
            "w-bob-1",
            "pg_llm_bob",
            "auto",
            TrustTier::Tier1Public,
            "single_worker",
        )
        .await
        .expect("admit req 3");
        let s3 = settle_llm_request_tx(
            &pool,
            "pg-req-3",
            a3.reserved_input_tokens + 10,
            10,
            0,
            None,
        )
        .await;
        assert!(matches!(
            s3,
            Err(LlmBillingError::Quota(QuotaError::Overrun))
        ));
        let s3_status: String = sqlx::query_scalar(
            "SELECT status FROM llm_inference_requests WHERE request_id = 'pg-req-3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(s3_status, "overrun_fault");

        // Verify token conservation: 1M - 100 - a3.reserved_input_tokens input remaining!
        let (in_avail, out_avail): (i64, i64) = sqlx::query_as(
            "SELECT input_available_tokens, output_available_tokens FROM llm_block_quotas WHERE owner = 'pg_llm_alice' AND model_id = 'qwen2.5-7b-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            in_avail as u64,
            TOKENS_PER_BLOCK - 100 - a3.reserved_input_tokens
        );
        assert_eq!(
            out_avail as u64,
            TOKENS_PER_BLOCK - 50 - a3.reserved_output_tokens
        );
    }
}
