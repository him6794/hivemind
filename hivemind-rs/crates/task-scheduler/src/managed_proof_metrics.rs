use std::sync::atomic::{AtomicU64, Ordering};

static VERIFICATION_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
static VERIFIED: AtomicU64 = AtomicU64::new(0);
static REJECTED: AtomicU64 = AtomicU64::new(0);
static QUEUE_RETRIES: AtomicU64 = AtomicU64::new(0);
static OBSERVE_FALLBACKS: AtomicU64 = AtomicU64::new(0);
static LEGACY_SETTLEMENTS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedProofMetricEvent {
    Verified,
    Rejected,
    QueueRetry,
    ObserveFallback,
    LegacySettlement,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManagedProofMetricsSnapshot {
    pub verification_attempts: u64,
    pub verified: u64,
    pub rejected: u64,
    pub queue_retries: u64,
    pub observe_fallbacks: u64,
    pub legacy_settlements: u64,
}

pub fn record(event: ManagedProofMetricEvent) {
    match event {
        ManagedProofMetricEvent::Verified => {
            VERIFICATION_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
            VERIFIED.fetch_add(1, Ordering::Relaxed);
        }
        ManagedProofMetricEvent::Rejected => {
            VERIFICATION_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
            REJECTED.fetch_add(1, Ordering::Relaxed);
        }
        ManagedProofMetricEvent::QueueRetry => {
            QUEUE_RETRIES.fetch_add(1, Ordering::Relaxed);
        }
        ManagedProofMetricEvent::ObserveFallback => {
            OBSERVE_FALLBACKS.fetch_add(1, Ordering::Relaxed);
        }
        ManagedProofMetricEvent::LegacySettlement => {
            LEGACY_SETTLEMENTS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn snapshot() -> ManagedProofMetricsSnapshot {
    ManagedProofMetricsSnapshot {
        verification_attempts: VERIFICATION_ATTEMPTS.load(Ordering::Relaxed),
        verified: VERIFIED.load(Ordering::Relaxed),
        rejected: REJECTED.load(Ordering::Relaxed),
        queue_retries: QUEUE_RETRIES.load(Ordering::Relaxed),
        observe_fallbacks: OBSERVE_FALLBACKS.load(Ordering::Relaxed),
        legacy_settlements: LEGACY_SETTLEMENTS.load(Ordering::Relaxed),
    }
}
