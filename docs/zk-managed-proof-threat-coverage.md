# Managed-Proof Threat Coverage

A Worker is untrusted. Everything it reports about a `managed-function-v0`
execution — the output, the resource usage, the receipt — is a claim. This
document maps each way a Worker could try to profit from lying to the test that
proves the Nodepool refuses it.

Every test named here is a normal `cargo test` unit test. Locations:

- `V` = `hivemind-rs/crates/managed-proof/src/lib.rs` (Nodepool verifier)
- `S` = `hivemind-rs/crates/task-scheduler/src/dispatcher.rs` (settlement)
- `B` = `hivemind-rs/crates/hivemind-bin/src/lib.rs` (isolated verifier process)

The verifier tests that touch real cryptography need `--features risc0-verifier`.

## Forging the billing figures

| Attack | Refused by | Test |
|---|---|---|
| Inflate `usage_units` inside the proven claim | Claim binding rejects usage above the task budget | `V claim_binding_rejects_usage_above_budget`, `V claim_rejects_usage_above_the_bound_budget` |
| Report large `managed_executed_ops` / `managed_output_bytes` alongside a valid proof | Only the verified claim reaches settlement; the Worker's own scalars are discarded | `S verified_claim_is_the_only_managed_settlement_source`, `S managed_completion_accepts_only_the_verifier_returned_claim` |
| Send a legacy receipt JSON instead of a proof | Managed tasks require a proof under `enforce` | `S managed_completion_requires_proof_even_with_legacy_receipt_fields`, `S test_execute_on_worker_rejects_managed_task_without_proof` |
| Claim a budget the task never had | Claim binding compares against the stored `max_cpt` | `V claim_binding_rejects_max_usage_units_mismatch`, `S verified_claim_rejects_budget_mismatch` |

## Forging what ran

| Attack | Refused by | Test |
|---|---|---|
| Prove a cheaper program than the one submitted | Source SHA-256 binding | `V claim_binding_rejects_source_mismatch`, `S verified_claim_rejects_source_mismatch` |
| Prove against a different input | Input SHA-256 binding | `V claim_binding_rejects_input_mismatch`, `S verified_claim_rejects_input_mismatch` |
| Return a different result than the one proven | Output SHA-256 and length binding | `V claim_binding_rejects_output_hash_mismatch`, `V claim_binding_rejects_output_bytes_mismatch`, `S verified_claim_rejects_output_mismatch` |
| Omit the input entirely | Missing input binds to an explicit `null`, not to "anything" | `S verified_claim_uses_null_for_missing_managed_input` |
| Settle a task with no stored source, or a non-positive budget | Contract checks before verification | `S verified_claim_rejects_missing_source_contract`, `S verified_claim_rejects_nonpositive_budget_contract` |

## Replay and version confusion

| Attack | Refused by | Test |
|---|---|---|
| Reuse a valid proof from another task | Task-ID binding | `V claim_binding_rejects_task_id_mismatch`, `S verified_claim_rejects_replay_for_another_task` |
| Submit a proof produced under an older protocol | Protocol version binding | `V claim_binding_rejects_protocol_version_mismatch`, `S verified_claim_rejects_protocol_version_mismatch` |
| Submit a proof from a different runtime or cost model | Runtime and cost-model ID binding | `V claim_binding_rejects_runtime_id_mismatch`, `V claim_binding_rejects_cost_model_id_mismatch`, `S verified_claim_rejects_runtime_version_mismatch`, `S verified_claim_rejects_cost_model_version_mismatch` |

## Attacking the proof itself

| Attack | Refused by | Test |
|---|---|---|
| Prove with a guest image the Worker chose | Image ID is pinned to `RISC0_MANAGED_GUEST_ID` | `V verifier_rejects_untrusted_image_id_before_receipt_decode` |
| Send a malformed or short image ID | Length check before decode | `V verifier_rejects_invalid_image_id_length_before_receipt_decode` |
| Claim a different proof scheme | Scheme is pinned, and the attacker string is not retained | `V verifier_rejects_untrusted_scheme_before_receipt_decode` |
| Send a dev-mode fake receipt | `disable-dev-mode` is compiled in, so a fake is rejected even if the environment variable is set | `V verifier_rejects_fake_receipt_when_dev_mode_is_disabled`, `V verifier_rejects_fake_receipt_without_panicking_when_dev_mode_env_is_set` |
| Tamper with the seal | Cryptographic verification | `V verifier_rejects_within_cap_tampered_composite_seal_at_crypto_gate`, `B verifier_rejects_an_invalid_cryptographic_proof` |
| Swap the journal for one describing a better outcome | Envelope journal must equal the receipt journal | `V verifier_rejects_envelope_journal_mismatch` |
| Hide work behind assumptions or extra segments | Only a single Composite segment with no assumptions is accepted | `V verifier_rejects_too_many_composite_segments_before_crypto`, `V verifier_rejects_empty_composite_before_crypto`, `V verifier_rejects_composite_assumption_receipts_before_crypto`, `V verifier_rejects_final_claim_assumptions_before_crypto` |
| Reorder or re-hash segments | Segment index and hash function are pinned | `V verifier_rejects_unexpected_segment_index_before_crypto`, `V verifier_rejects_unsupported_segment_hash_function_before_crypto` |

## Attacking the verifier as a resource

The verifier runs in a bounded child process, so a Worker cannot turn
verification into a denial of service against the Nodepool.

| Attack | Refused by | Test |
|---|---|---|
| Oversized journal | 4 KiB cap before decode | `V verifier_rejects_oversized_journal_before_receipt_decode` |
| Oversized receipt | 2 MiB cap before JSON decode | `V verifier_rejects_oversized_receipt_before_json_decode` |
| Oversized seal | 131,072-word cap before crypto | `V verifier_rejects_oversized_segment_seal_before_crypto` |
| Malformed receipt JSON | Rejected before crypto | `V verifier_rejects_invalid_receipt_json`, `B verifier_rejects_a_malformed_envelope` |
| Oversized RPC message | Whole-response caps applied before proof handling | `S worker_response_rejects_oversized_status_message`, `S worker_response_rejects_oversized_legacy_receipt` |
| Expensive invalid claim | Claim validity checked before cryptography | `V verifier_rejects_invalid_claim_before_crypto_on_public_path`, `V verifier_rejects_invalid_verified_claim_journal` |

## What is deliberately not defended

A Worker can always refuse to work, return a failure, or return a valid proof
slowly. Those cost it the task and its reputation; they do not let it be paid
for work it did not do.

Verifier saturation is treated as the Nodepool's own backpressure, not Worker
misbehaviour: a full local queue redispatches the task without recording a
worker failure (`S verifier_local_queue_pressure_retries_without_blame`). The
same holds for transport failures (`S unavailable_is_redispatchable_without_worker_penalty`,
`S connect_transport_error_is_redispatchable_without_worker_penalty`).

`MANAGED_PROOF_ROLLOUT_MODE` values other than `enforce` intentionally settle
from Worker-reported numbers, so none of the guarantees above hold under
`observe` or `off`. Both record a `managed_proof_verification` audit entry on
every settlement precisely so that the window is visible after the fact
(`S test_execute_on_worker_off_mode_settles_managed_task_and_audits_legacy`).
