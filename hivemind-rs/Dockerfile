FROM rust:1-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY proto ./proto
COPY hivemind-rs ./hivemind-rs

WORKDIR /app/hivemind-rs
RUN cargo build --release --bin hivemind-bin

FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/hivemind-rs/target/release/hivemind-bin /app/hivemind-bin

RUN mkdir -p /app/api/torrents /app/bt_torrents /app/sandbox

EXPOSE 8082 50051 50053

CMD ["/app/hivemind-bin", "all"]
