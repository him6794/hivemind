#!/usr/bin/env bash
set -euo pipefail

# Build the portable userspace Tailscale binaries used by the Windows clients.
# Pin this version and update it deliberately when the client runtime is tested.
TAILSCALE_VERSION="${TAILSCALE_VERSION:-v1.98.9}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SCRIPT_DIR}/.."
OUT_DIR="${TAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/tailscale/windows-x86_64}"
SRC_DIR="${ROOT_DIR}/.cache/tailscale-${TAILSCALE_VERSION}"

command -v go >/dev/null || { echo "go is required to build Tailscale" >&2; exit 1; }
mkdir -p "${ROOT_DIR}/.cache" "${OUT_DIR}"

if [[ ! -d "${SRC_DIR}/.git" ]]; then
  git clone --depth 1 --branch "${TAILSCALE_VERSION}" \
    https://github.com/tailscale/tailscale.git "${SRC_DIR}"
fi

pushd "${SRC_DIR}" >/dev/null
GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -o "${OUT_DIR}/tailscale.exe" ./cmd/tailscale
GOOS=windows GOARCH=amd64 CGO_ENABLED=0 go build -trimpath -o "${OUT_DIR}/tailscaled.exe" ./cmd/tailscaled
popd >/dev/null

echo "Windows Tailscale runtime written to ${OUT_DIR}"
ls -lh "${OUT_DIR}/tailscale.exe" "${OUT_DIR}/tailscaled.exe"
