FROM rust:1.96-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY proto ./proto
COPY hivemind-rs ./hivemind-rs
COPY executor-rs/README.md ./executor-rs/README.md
COPY executor-rs/Cargo.toml executor-rs/Cargo.lock ./executor-rs/
COPY executor-rs/crates/managed-function-runtime ./executor-rs/crates/managed-function-runtime
COPY executor-rs/crates/managed-function-transpiler ./executor-rs/crates/managed-function-transpiler
COPY executor-rs/crates/monty ./executor-rs/crates/monty
COPY executor-rs/crates/monty-cli ./executor-rs/crates/monty-cli
COPY executor-rs/crates/monty-js ./executor-rs/crates/monty-js
COPY executor-rs/crates/monty-type-checking ./executor-rs/crates/monty-type-checking
COPY executor-rs/crates/monty-typeshed ./executor-rs/crates/monty-typeshed

WORKDIR /app/hivemind-rs
ARG HIVEMIND_BIN=hivemind-bin
ARG HIVEMIND_FEATURES=master,nodepool,worker,cli
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/hivemind-rs/target \
    cargo build --locked --release --bin ${HIVEMIND_BIN} --no-default-features --features ${HIVEMIND_FEATURES} \
    && cp /app/hivemind-rs/target/release/${HIVEMIND_BIN} /tmp/hivemind-bin

WORKDIR /app/executor-rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/executor-rs/target \
    cargo build --locked --release --bin monty \
    && cp /app/executor-rs/target/release/monty /tmp/monty

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

FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

ARG HIVEMIND_BIN=hivemind-bin
COPY --from=builder /tmp/hivemind-bin /app/hivemind-bin
COPY --from=builder /tmp/monty /app/monty
COPY --from=master-ui /ui/dist /app/frontend/master-ui/dist
COPY --from=worker-ui /ui/dist /app/frontend/worker-ui/dist

RUN mkdir -p /app/api/torrents /app/bt_torrents /app/sandbox
RUN groupadd --system --gid 10001 hivemind \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin hivemind \
    && chown -R 10001:10001 /app

USER 10001:10001
ENV MONTY_EXECUTABLE=/app/monty

EXPOSE 8082 8090 50051 50053 18080 6969 6881

CMD ["/app/hivemind-bin"]
