#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="/opt/hivemind-worker"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

BIN_DIR="$INSTALL_DIR/bin"
RELEASE_DIR="$INSTALL_DIR/release"
SRC="$RELEASE_DIR/worker-executor"
DST="$BIN_DIR/worker-executor"

if [[ ! -f "$SRC" ]]; then
  echo "Missing release artifact: $SRC" >&2
  exit 1
fi

mkdir -p "$BIN_DIR"
cp "$SRC" "$DST"
chmod +x "$DST"

if [[ -f "$RELEASE_DIR/version.txt" ]]; then
  echo "Updated worker to version $(tr -d '\r\n' < "$RELEASE_DIR/version.txt")"
else
  echo "Updated worker binary (version metadata missing)"
fi
