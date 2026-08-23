# syntax=docker/dockerfile:1.7

# The pinned RISC Zero sidecar cannot be compiled inside this image: the guest
# image ID depends on the exact risc0 guest toolchain that produced the Nodepool
# trust pin, and a container-built toolchain produces a different guest whose
# every proof the Nodepool rejects (see
# docs/zk-managed-proof-build-attestation.md). The operator therefore builds the
# sidecar once with scripts/build-managed-prover.sh — which refuses to stage a
# binary unless its environment reproduces the pinned guest image ID — and
# passes the staged file to this build:
#
#   docker build \
#     --build-arg MANAGED_PROVER_SIDECAR=publish/hivemind-managed-proof-prover \
#     -f packaging/managed-prover-service/Dockerfile .
#
FROM rust:1.96-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Keep this image focused on the provider crate while preserving the path
# dependencies used by the Hivemind workspace.
COPY proto ./proto
COPY hivemind-rs/Cargo.toml hivemind-rs/Cargo.lock ./hivemind-rs/
COPY hivemind-rs/.cargo ./hivemind-rs/.cargo
COPY hivemind-rs/crates ./hivemind-rs/crates
COPY executor-rs/Cargo.toml executor-rs/Cargo.lock ./executor-rs/
COPY executor-rs/crates/managed-function-runtime ./executor-rs/crates/managed-function-runtime
COPY executor-rs/crates/general-compute-runtime ./executor-rs/crates/general-compute-runtime

WORKDIR /app/hivemind-rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/hivemind-rs/target \
    cargo install --locked --path crates/managed-prover-service --root /app/provider-bin

# The temporary operator deployment targets the Docker Desktop Linux amd64 engine.
# Target platform: linux/amd64.
FROM debian:trixie-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 hivemind-prover \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin hivemind-prover \
    && mkdir -p /app/bin /app/prover /etc/hivemind-managed-prover /var/lib/hivemind-managed-prover \
    && chown -R 10001:10001 /app /etc/hivemind-managed-prover /var/lib/hivemind-managed-prover

WORKDIR /app

COPY --from=builder /app/provider-bin/bin/hivemind-managed-prover-service /app/bin/hivemind-managed-prover-service
# BuildKit fails this COPY with an explicit "not found" when the operator has
# not staged the sidecar yet, instead of silently shipping a provider without
# a prover. A missing sidecar must never look like a successful build.
COPY packaging/managed-prover/hivemind-managed-proof-prover /app/prover/hivemind-managed-proof-prover

RUN chmod 0755 /app/bin/hivemind-managed-prover-service /app/prover/hivemind-managed-proof-prover \
    && chown 10001:10001 /app/bin/hivemind-managed-prover-service /app/prover/hivemind-managed-proof-prover

ENV MANAGED_PROOF_AUTH_PUBLIC_KEY_PEM="" \
    MANAGED_PROVER_SERVICE_ADDR=0.0.0.0:50054 \
    MANAGED_PROVER_SERVICE_EXECUTABLE=/app/prover/hivemind-managed-proof-prover \
    MANAGED_PROVER_STATE_DIR=/var/lib/hivemind-managed-prover \
    MANAGED_PROVER_QUEUE_CAPACITY=1 \
    MANAGED_PROVER_TIMEOUT_SECS=1200 \
    MANAGED_PROVER_TLS_SERVER_CERT_PATH=/etc/hivemind-managed-prover/server.crt \
    MANAGED_PROVER_TLS_SERVER_KEY_PATH=/etc/hivemind-managed-prover/server.key \
    MANAGED_PROVER_TLS_CLIENT_CA_PATH=/etc/hivemind-managed-prover/worker-client-ca.crt \
    HOME=/tmp

USER 10001:10001
EXPOSE 50054

ENTRYPOINT ["/app/bin/hivemind-managed-prover-service"]
