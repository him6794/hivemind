FROM rust:1.96-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY proto ./proto
COPY hivemind-rs ./hivemind-rs
COPY executor-rs/Cargo.toml executor-rs/Cargo.lock ./executor-rs/
COPY executor-rs/crates/managed-function-runtime ./executor-rs/crates/managed-function-runtime

WORKDIR /app/hivemind-rs
ARG HIVEMIND_BIN=hivemind-bin
ARG HIVEMIND_FEATURES=master,nodepool,worker,cli
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/hivemind-rs/target \
    cargo build --locked --release --bin ${HIVEMIND_BIN} --no-default-features --features ${HIVEMIND_FEATURES} \
    && cp /app/hivemind-rs/target/release/${HIVEMIND_BIN} /tmp/hivemind-bin

# UI assets are architecture-independent. Build them on the native builder
# platform so cross-compiling the ARM runtime does not execute Node under QEMU.
FROM --platform=$BUILDPLATFORM node:20-alpine AS master-ui
WORKDIR /ui
COPY frontend/master-ui/package*.json ./
RUN npm ci --include=dev
COPY frontend/master-ui/ ./
RUN npm run build

FROM --platform=$BUILDPLATFORM node:20-alpine AS worker-ui
WORKDIR /ui
COPY frontend/worker-ui/package*.json ./
RUN npm ci --include=dev
COPY frontend/worker-ui/ ./
RUN npm run build

# The managed-proof sidecar is built on the supported Linux prover host and
# currently requires glibc 2.39. Trixie supplies a newer compatible runtime;
# bookworm's glibc 2.36 made the packaged sidecar fail before parsing input.
FROM debian:trixie-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

ARG HIVEMIND_BIN=hivemind-bin
COPY --from=builder /tmp/hivemind-bin /app/hivemind-bin
COPY --from=master-ui /ui/dist /app/frontend/master-ui/dist
COPY --from=worker-ui /ui/dist /app/frontend/worker-ui/dist

# Optional managed-proof prover sidecar. RISC Zero only supports Linux/macOS
# prover hosts and needs a dedicated guest toolchain, so the binary is built
# once by scripts/build-managed-prover.sh and staged in packaging/managed-prover
# rather than compiled here. The directory always exists, so the image builds
# with or without it; without the binary every managed task fails closed because
# MANAGED_PROVER_EXECUTABLE cannot be spawned.
COPY packaging/managed-prover/ /app/prover/
RUN chmod -R a+rX /app/prover

RUN mkdir -p /app/api/torrents /app/bt_torrents /app/sandbox
RUN groupadd --system --gid 10001 hivemind \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin hivemind \
    && chown -R 10001:10001 /app

USER 10001:10001

EXPOSE 8082 8090 50051 50053 18080 6969 6881

CMD ["/app/hivemind-bin"]
