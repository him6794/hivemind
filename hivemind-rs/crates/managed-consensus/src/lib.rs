use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Version of the Nodepool-coordinated managed execution quorum protocol.
pub const CONSENSUS_PROTOCOL_VERSION: u16 = 1;
/// Evidence produced by this crate means agreement between authenticated Workers,
/// not independent correctness validation of the agreed result.
pub const CONSENSUS_EVIDENCE_LEVEL: &str = "replicated";
pub const MAX_REPLICAS: u16 = 7;
pub const MAX_CERTIFICATE_OBSERVATIONS: usize = 7;
/// Canonical semantic identity used when the frozen managed-function-v0 task
/// does not carry an explicit production backend registration.
pub const MANAGED_DSL_DEFAULT_BACKEND_ID: &str = "managed-function-v0";
pub const MANAGED_DSL_DEFAULT_SEMANTICS_DIGEST: &str =
    "sha256:d61a8134f665100855402d7455cfcf3b3e701a79ad43e0039f4ad6c5f05bafef";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusBinding {
    pub task_id: String,
    pub execution_id: String,
    pub round_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub runtime: String,
    pub backend_id: String,
    pub semantics_digest: String,
    pub source_digest: String,
    pub input_digest: String,
}

impl ConsensusBinding {
    pub fn validate(&self) -> Result<(), ConsensusError> {
        for (name, value) in [
            ("task_id", &self.task_id),
            ("execution_id", &self.execution_id),
            ("round_id", &self.round_id),
            ("idempotency_key", &self.idempotency_key),
            ("request_digest", &self.request_digest),
            ("runtime", &self.runtime),
            ("backend_id", &self.backend_id),
            ("semantics_digest", &self.semantics_digest),
            ("source_digest", &self.source_digest),
            ("input_digest", &self.input_digest),
        ] {
            if value.trim().is_empty() {
                return Err(ConsensusError::MissingBinding(name));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusObservation {
    pub worker_id: String,
    pub replica_id: String,
    pub attempt_id: String,
    pub binding: ConsensusBinding,
    pub success: bool,
    pub output_digest: [u8; 32],
    pub result_digest: [u8; 32],
    pub output_bytes: u64,
    /// These values are retained for diagnostics only. They are deliberately
    /// excluded from the accepted result digest and never authorize billing.
    pub claimed_usage_units: u64,
    pub claimed_executed_ops: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusCertificate {
    pub protocol_version: u16,
    pub evidence_level: String,
    pub binding: ConsensusBinding,
    pub required_quorum: u16,
    pub replica_count: u16,
    pub matching_count: u16,
    pub result_digest: [u8; 32],
    pub output_digest: [u8; 32],
    pub output_bytes: u64,
    pub participants: Vec<ConsensusParticipant>,
    pub certificate_digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusParticipant {
    pub worker_id: String,
    pub replica_id: String,
    pub attempt_id: String,
    pub output_digest: [u8; 32],
    pub result_digest: [u8; 32],
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuorumPolicy {
    pub replica_count: u16,
    pub required_quorum: u16,
}

impl QuorumPolicy {
    pub fn new(replica_count: u16, required_quorum: u16) -> Result<Self, ConsensusError> {
        let policy = Self {
            replica_count,
            required_quorum,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(self) -> Result<(), ConsensusError> {
        if !(2..=MAX_REPLICAS).contains(&self.replica_count) {
            return Err(ConsensusError::InvalidReplicaCount(self.replica_count));
        }
        if self.required_quorum <= self.replica_count / 2
            || self.required_quorum > self.replica_count
        {
            return Err(ConsensusError::InvalidQuorum {
                replica_count: self.replica_count,
                required_quorum: self.required_quorum,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConsensusError {
    #[error("consensus binding field {0} is empty")]
    MissingBinding(&'static str),
    #[error("invalid replica count: {0}")]
    InvalidReplicaCount(u16),
    #[error("invalid quorum {required_quorum} for {replica_count} replicas")]
    InvalidQuorum {
        replica_count: u16,
        required_quorum: u16,
    },
    #[error("too many observations: {received} exceeds {limit}")]
    TooManyObservations { received: usize, limit: usize },
    #[error("observation from Worker {0} is duplicated")]
    DuplicateWorker(String),
    #[error("observation for replica {0} is duplicated")]
    DuplicateReplica(String),
    #[error("observation from replica {replica_id} has mismatched {field}")]
    BindingMismatch {
        replica_id: String,
        field: &'static str,
    },
    #[error("observation from replica {0} has an empty identity")]
    EmptyObservationIdentity(String),
    #[error("no result reached quorum")]
    NoQuorum,
    #[error("more than one result reached quorum")]
    AmbiguousQuorum,
}

/// Evaluate observations collected by Nodepool. The caller must obtain the
/// observations through authenticated, Nodepool-issued replica assignments;
/// this function intentionally treats the Worker ID as an identity label, not
/// as a signature or an independent trust root.
pub fn evaluate_quorum(
    binding: &ConsensusBinding,
    policy: QuorumPolicy,
    observations: &[ConsensusObservation],
) -> Result<ConsensusCertificate, ConsensusError> {
    binding.validate()?;
    policy.validate()?;
    if observations.len() > MAX_CERTIFICATE_OBSERVATIONS {
        return Err(ConsensusError::TooManyObservations {
            received: observations.len(),
            limit: MAX_CERTIFICATE_OBSERVATIONS,
        });
    }
    if observations.len() > policy.replica_count as usize {
        return Err(ConsensusError::TooManyObservations {
            received: observations.len(),
            limit: policy.replica_count as usize,
        });
    }

    let mut workers = BTreeSet::new();
    let mut replicas = BTreeSet::new();
    let mut matching: BTreeMap<[u8; 32], Vec<&ConsensusObservation>> = BTreeMap::new();

    for observation in observations {
        if observation.worker_id.trim().is_empty()
            || observation.replica_id.trim().is_empty()
            || observation.attempt_id.trim().is_empty()
        {
            return Err(ConsensusError::EmptyObservationIdentity(
                observation.replica_id.clone(),
            ));
        }
        if !workers.insert(observation.worker_id.clone()) {
            return Err(ConsensusError::DuplicateWorker(
                observation.worker_id.clone(),
            ));
        }
        if !replicas.insert(observation.replica_id.clone()) {
            return Err(ConsensusError::DuplicateReplica(
                observation.replica_id.clone(),
            ));
        }
        validate_observation_binding(binding, observation)?;
        if observation.success {
            matching
                .entry(observation.result_digest)
                .or_default()
                .push(observation);
        }
    }

    let winners: Vec<_> = matching
        .values()
        .filter(|group| group.len() >= policy.required_quorum as usize)
        .collect();
    if winners.is_empty() {
        return Err(ConsensusError::NoQuorum);
    }
    if winners.len() > 1 {
        return Err(ConsensusError::AmbiguousQuorum);
    }

    let winner = winners[0];
    let result_digest = winner[0].result_digest;
    let output_digest = winner[0].output_digest;
    let output_bytes = winner[0].output_bytes;
    if winner.iter().any(|observation| {
        observation.output_digest != output_digest || observation.output_bytes != output_bytes
    }) {
        return Err(ConsensusError::BindingMismatch {
            replica_id: winner[0].replica_id.clone(),
            field: "output_bytes",
        });
    }

    let mut participants: Vec<_> = winner
        .iter()
        .map(|observation| ConsensusParticipant {
            worker_id: observation.worker_id.clone(),
            replica_id: observation.replica_id.clone(),
            attempt_id: observation.attempt_id.clone(),
            output_digest: observation.output_digest,
            result_digest: observation.result_digest,
            output_bytes: observation.output_bytes,
        })
        .collect();
    participants.sort_by(|left, right| {
        left.replica_id
            .cmp(&right.replica_id)
            .then_with(|| left.worker_id.cmp(&right.worker_id))
    });

    let mut certificate = ConsensusCertificate {
        protocol_version: CONSENSUS_PROTOCOL_VERSION,
        evidence_level: CONSENSUS_EVIDENCE_LEVEL.to_owned(),
        binding: binding.clone(),
        required_quorum: policy.required_quorum,
        replica_count: policy.replica_count,
        matching_count: u16::try_from(participants.len()).expect("quorum is bounded"),
        result_digest,
        output_digest,
        output_bytes,
        participants,
        certificate_digest: [0; 32],
    };
    certificate.certificate_digest = certificate_digest(&certificate);
    Ok(certificate)
}

fn validate_observation_binding(
    expected: &ConsensusBinding,
    observation: &ConsensusObservation,
) -> Result<(), ConsensusError> {
    let actual = &observation.binding;
    for (field, expected_value, actual_value) in [
        ("task_id", &expected.task_id, &actual.task_id),
        ("execution_id", &expected.execution_id, &actual.execution_id),
        ("round_id", &expected.round_id, &actual.round_id),
        (
            "idempotency_key",
            &expected.idempotency_key,
            &actual.idempotency_key,
        ),
        (
            "request_digest",
            &expected.request_digest,
            &actual.request_digest,
        ),
        ("runtime", &expected.runtime, &actual.runtime),
        ("backend_id", &expected.backend_id, &actual.backend_id),
        (
            "semantics_digest",
            &expected.semantics_digest,
            &actual.semantics_digest,
        ),
        (
            "source_digest",
            &expected.source_digest,
            &actual.source_digest,
        ),
        ("input_digest", &expected.input_digest, &actual.input_digest),
    ] {
        if expected_value != actual_value {
            return Err(ConsensusError::BindingMismatch {
                replica_id: observation.replica_id.clone(),
                field,
            });
        }
    }
    Ok(())
}

/// Hash the certificate using an explicit length-prefixed encoding rather than
/// relying on a map or platform-specific serializer implementation.
pub fn certificate_digest(certificate: &ConsensusCertificate) -> [u8; 32] {
    let mut bytes = Vec::new();
    put_u16(&mut bytes, certificate.protocol_version);
    put_str(&mut bytes, &certificate.evidence_level);
    put_binding(&mut bytes, &certificate.binding);
    put_u16(&mut bytes, certificate.required_quorum);
    put_u16(&mut bytes, certificate.replica_count);
    put_u16(&mut bytes, certificate.matching_count);
    bytes.extend_from_slice(&certificate.result_digest);
    bytes.extend_from_slice(&certificate.output_digest);
    put_u64(&mut bytes, certificate.output_bytes);
    put_u16(
        &mut bytes,
        u16::try_from(certificate.participants.len()).expect("participants are bounded"),
    );
    for participant in &certificate.participants {
        put_str(&mut bytes, &participant.worker_id);
        put_str(&mut bytes, &participant.replica_id);
        put_str(&mut bytes, &participant.attempt_id);
        bytes.extend_from_slice(&participant.output_digest);
        bytes.extend_from_slice(&participant.result_digest);
        put_u64(&mut bytes, participant.output_bytes);
    }
    Sha256::digest(bytes).into()
}

pub fn output_digest(output: &[u8]) -> [u8; 32] {
    Sha256::digest(output).into()
}

pub fn digest_hex(value: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(value).into();
    let mut encoded = String::with_capacity(71);
    encoded.push_str("sha256:");
    for byte in digest {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

fn put_binding(bytes: &mut Vec<u8>, binding: &ConsensusBinding) {
    for value in [
        &binding.task_id,
        &binding.execution_id,
        &binding.round_id,
        &binding.idempotency_key,
        &binding.request_digest,
        &binding.runtime,
        &binding.backend_id,
        &binding.semantics_digest,
        &binding.source_digest,
        &binding.input_digest,
    ] {
        put_str(bytes, value);
    }
}

fn put_str(bytes: &mut Vec<u8>, value: &str) {
    put_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

fn put_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ConsensusBinding {
        ConsensusBinding {
            task_id: "task-1".into(),
            execution_id: "exec-1".into(),
            round_id: "round-1".into(),
            idempotency_key: "idem-1".into(),
            request_digest: "sha256:req".into(),
            runtime: "managed-function-v0".into(),
            backend_id: "managed-default".into(),
            semantics_digest: "sha256:semantics".into(),
            source_digest: "sha256:source".into(),
            input_digest: "sha256:input".into(),
        }
    }

    fn observation(worker_id: &str, replica_id: &str, output: &[u8]) -> ConsensusObservation {
        ConsensusObservation {
            worker_id: worker_id.into(),
            replica_id: replica_id.into(),
            attempt_id: format!("attempt-{replica_id}"),
            binding: binding(),
            success: true,
            output_digest: output_digest(output),
            result_digest: output_digest(output),
            output_bytes: output.len() as u64,
            claimed_usage_units: 9,
            claimed_executed_ops: 9,
        }
    }

    #[test]
    fn two_matching_observations_form_a_certificate() {
        let policy = QuorumPolicy::new(3, 2).unwrap();
        let observations = vec![
            observation("worker-a", "replica-a", b"same"),
            observation("worker-b", "replica-b", b"same"),
            observation("worker-c", "replica-c", b"different"),
        ];

        let certificate = evaluate_quorum(&binding(), policy, &observations).unwrap();

        assert_eq!(certificate.matching_count, 2);
        assert_eq!(certificate.output_digest, output_digest(b"same"));
        assert_eq!(certificate.evidence_level, CONSENSUS_EVIDENCE_LEVEL);
        assert_eq!(
            certificate.certificate_digest,
            certificate_digest(&certificate)
        );
    }

    #[test]
    fn no_quorum_never_accepts_a_single_result() {
        let policy = QuorumPolicy::new(3, 2).unwrap();
        let observations = vec![observation("worker-a", "replica-a", b"only")];

        assert_eq!(
            evaluate_quorum(&binding(), policy, &observations),
            Err(ConsensusError::NoQuorum)
        );
    }

    #[test]
    fn duplicate_worker_is_rejected_even_with_different_replica_ids() {
        let policy = QuorumPolicy::new(3, 2).unwrap();
        let observations = vec![
            observation("worker-a", "replica-a", b"same"),
            observation("worker-a", "replica-b", b"same"),
        ];

        assert_eq!(
            evaluate_quorum(&binding(), policy, &observations),
            Err(ConsensusError::DuplicateWorker("worker-a".into()))
        );
    }

    #[test]
    fn mismatched_request_identity_is_rejected() {
        let policy = QuorumPolicy::new(3, 2).unwrap();
        let mut changed = observation("worker-a", "replica-a", b"same");
        changed.binding.request_digest = "sha256:other".into();

        assert_eq!(
            evaluate_quorum(&binding(), policy, &[changed]),
            Err(ConsensusError::BindingMismatch {
                replica_id: "replica-a".into(),
                field: "request_digest",
            })
        );
    }

    #[test]
    fn invalid_policy_fails_closed_before_evaluating_votes() {
        let policy = QuorumPolicy {
            replica_count: 4,
            required_quorum: 2,
        };
        let observations = vec![
            observation("worker-a", "replica-a", b"left"),
            observation("worker-b", "replica-b", b"left"),
        ];

        assert_eq!(
            evaluate_quorum(&binding(), policy, &observations),
            Err(ConsensusError::InvalidQuorum {
                replica_count: 4,
                required_quorum: 2,
            })
        );
    }

    #[test]
    fn certificate_digest_is_order_independent_for_participants() {
        let policy = QuorumPolicy::new(3, 2).unwrap();
        let a = observation("worker-a", "replica-a", b"same");
        let b = observation("worker-b", "replica-b", b"same");
        let first = evaluate_quorum(&binding(), policy, &[a.clone(), b.clone()]).unwrap();
        let second = evaluate_quorum(&binding(), policy, &[b, a]).unwrap();

        assert_eq!(first.certificate_digest, second.certificate_digest);
    }
}
