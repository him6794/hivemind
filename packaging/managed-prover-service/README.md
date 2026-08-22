# Managed prover service

`hivemind-managed-prover-service` is the operator-managed Linux proving endpoint for
native Windows Workers. It is not a compatibility layer: the Worker executes the
closed DSL natively on Windows and sends only an authenticated, task-bound proof
request to this service.

The service must run on a separate Linux x86_64 host with the pinned
`hivemind-managed-proof-prover` executable. It must not run on the Orange Pi
control-plane host. Orange Pi remains limited to Nodepool, Website API,
Headscale, PostgreSQL, and Redis.

## Build

Build the pinned sidecar on the approved Linux proving host:

```bash
bash scripts/build-managed-prover.sh
cargo build --release --locked \
  --manifest-path hivemind-rs/Cargo.toml \
  -p hivemind-managed-prover-service
```

The service executable is `target/release/hivemind-managed-prover-service` (the
package's default binary target name). Copy the executable and the staged
`hivemind-managed-proof-prover` to the operator host. Do not copy a Nodepool
private key, database credential, Headscale key, or reusable proof token.

## Required configuration

Set these variables in a protected service environment or an operator-owned
configuration file:

```text
MANAGED_PROOF_AUTH_PUBLIC_KEY_PEM=<Nodepool proof-auth Ed25519 public key>
MANAGED_PROVER_SERVICE_EXECUTABLE=/opt/hivemind/prover/hivemind-managed-proof-prover
MANAGED_PROVER_SERVICE_ADDR=0.0.0.0:50054
MANAGED_PROVER_STATE_DIR=/var/lib/hivemind-managed-prover
MANAGED_PROVER_QUEUE_CAPACITY=1
MANAGED_PROVER_TIMEOUT_SECS=1200
MANAGED_PROVER_TLS_SERVER_CERT_PATH=/etc/hivemind-managed-prover/server.crt
MANAGED_PROVER_TLS_SERVER_KEY_PATH=/etc/hivemind-managed-prover/server.key
MANAGED_PROVER_TLS_CLIENT_CA_PATH=/etc/hivemind-managed-prover/worker-client-ca.crt
```

The service refuses to start without the proof authorization public key, the
sidecar path, and all three mTLS server/client-CA files. The private signing key
stays on Nodepool and is never installed here.

The state directory contains only bounded job identity/binding, state, and proof
result data. It never stores the authorization JWT, source, or input. Pending
work is marked retryable after a service restart and must be submitted again;
completed results remain available subject to the bounded retention policy.

Restrict port `50054` to the Worker/provider network. Do not expose it to the
public Internet. Nodepool independently verifies every returned receipt and is
still the only usage, billing, settlement, and audit authority.

## Deployment boundary

- Native Windows Worker: closed-DSL execution plus mTLS client and per-attempt
  Nodepool authorization metadata.
- Linux x86_64 proving host: this service plus the pinned RISC Zero sidecar.
- Orange Pi: Nodepool, Website API, Headscale, PostgreSQL, and Redis only.
- Master and Worker: run on their own hosts; never deploy them to Orange Pi.

A provider outage, capability mismatch, invalid authorization, deadline, queue
saturation, malformed response, or failed receipt keeps managed execution
unsettled. Production remains `MANAGED_PROOF_ROLLOUT_MODE=enforce`; do not use
`off` or `observe` to bypass proof generation.
