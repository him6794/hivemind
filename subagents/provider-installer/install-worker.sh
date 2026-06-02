#!/usr/bin/env bash
set -euo pipefail

MASTER_URL=""
AUTH_TOKEN=""
INSTALL_DIR="/opt/hivemind-worker"
MAX_CPU_PERCENT="80"
MAX_MEMORY_MB="4096"
MAX_CONCURRENT_TASKS="2"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --master-url) MASTER_URL="$2"; shift 2 ;;
    --auth-token) AUTH_TOKEN="$2"; shift 2 ;;
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    --max-cpu-percent) MAX_CPU_PERCENT="$2"; shift 2 ;;
    --max-memory-mb) MAX_MEMORY_MB="$2"; shift 2 ;;
    --max-concurrent-tasks) MAX_CONCURRENT_TASKS="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

if [[ -z "$MASTER_URL" || -z "$AUTH_TOKEN" ]]; then
  echo "Usage: $0 --master-url <url> --auth-token <token> [--install-dir <dir>]" >&2
  exit 1
fi

BIN_DIR="$INSTALL_DIR/bin"
CFG_DIR="$INSTALL_DIR/config"
LOG_DIR="$INSTALL_DIR/logs"
RELEASE_DIR="$INSTALL_DIR/release"

mkdir -p "$BIN_DIR" "$CFG_DIR" "$LOG_DIR"

cat > "$CFG_DIR/worker.env" <<EOF
MASTER_HTTP_ADDR=$MASTER_URL
WORKER_AUTH_TOKEN=$AUTH_TOKEN
EXECUTOR_MAX_CPU_PERCENT=$MAX_CPU_PERCENT
EXECUTOR_MAX_MEMORY_MB=$MAX_MEMORY_MB
EXECUTOR_MAX_CONCURRENT_TASKS=$MAX_CONCURRENT_TASKS
EXECUTOR_SANDBOX_MODE=production
EXECUTOR_NETWORK_EGRESS_ENABLED=true
EXECUTOR_NETWORK_EGRESS_MODE=allowlist
EXECUTOR_NETWORK_EGRESS_TARGETS=8.8.8.8,1.1.1.1
EOF

if [[ -f "$RELEASE_DIR/worker-executor" ]]; then
  cp "$RELEASE_DIR/worker-executor" "$BIN_DIR/worker-executor"
  chmod +x "$BIN_DIR/worker-executor"
else
  echo "Warning: missing release artifact: $RELEASE_DIR/worker-executor" >&2
fi

if [[ -f "$RELEASE_DIR/version.txt" ]]; then
  echo "Installed version: $(tr -d '\r\n' < "$RELEASE_DIR/version.txt")"
else
  echo "Installed version: unknown (version.txt missing)"
fi

echo "Worker scaffold installed."
