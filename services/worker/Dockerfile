# Build stage
FROM golang:1.21-alpine AS builder

WORKDIR /build

# Install build dependencies
RUN apk add --no-cache git make gcc musl-dev linux-headers

# Copy go mod files
COPY go.mod go.sum ./
RUN go mod download

# Copy source code
COPY . .

# Build the worker binary
RUN CGO_ENABLED=0 GOOS=linux go build -a -installsuffix cgo -o worker ./cmd/worker

# Build the VPN demo binary
RUN CGO_ENABLED=0 GOOS=linux go build -a -installsuffix cgo -o vpn_demo ./examples/vpn_demo.go

# Runtime stage
FROM alpine:latest

RUN apk --no-cache add ca-certificates tzdata iptables iproute2 curl

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /build/worker .
COPY --from=builder /build/vpn_demo .

# Create directories for VPN state
RUN mkdir -p /var/lib/hivemind-vpn /var/lib/hivemind

# Expose ports
EXPOSE 50052

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost:50052/health || exit 1

CMD ["./worker"]
