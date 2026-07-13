FROM rust:1-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY proto ./proto
COPY hivemind-rs ./hivemind-rs
COPY executor-rs ./executor-rs

WORKDIR /app/hivemind-rs
ARG HIVEMIND_BIN=hivemind-bin
ARG HIVEMIND_FEATURES=master,nodepool,worker,cli
RUN cargo build --release --bin ${HIVEMIND_BIN} --no-default-features --features ${HIVEMIND_FEATURES}

FROM node:20-alpine AS master-ui
WORKDIR /ui
COPY frontend/master-ui/package*.json ./
RUN npm ci --include=dev
COPY frontend/master-ui/ ./
RUN npm run build

FROM node:20-alpine AS worker-ui
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
COPY --from=builder /app/hivemind-rs/target/release/${HIVEMIND_BIN} /app/hivemind
COPY --from=master-ui /ui/dist /app/frontend/master-ui/dist
COPY --from=worker-ui /ui/dist /app/frontend/worker-ui/dist

RUN mkdir -p /app/api/torrents /app/bt_torrents /app/sandbox
RUN groupadd --system --gid 10001 hivemind \
    && useradd --system --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin hivemind \
    && chown -R 10001:10001 /app

USER 10001:10001

EXPOSE 8082 50051 50053 18080 6969 6881

CMD ["/app/hivemind"]
