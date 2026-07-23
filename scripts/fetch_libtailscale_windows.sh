#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SCRIPT_DIR}/.."
VERSION="${LIBTAILSCALE_VERSION:-main}"
SRC_DIR="${ROOT_DIR}/.cache/libtailscale-${VERSION//\//_}"
OUT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-x86_64}"

if [[ ! -d "${SRC_DIR}/.git" ]]; then
  mkdir -p "${ROOT_DIR}/.cache"
  git clone --depth 1 --branch "${VERSION}" https://github.com/tailscale/libtailscale.git "${SRC_DIR}"
fi

# The upstream C archive currently uses Unix socketpair/FD-passing code. The
# Windows client only needs tsnet.Up and tsnet.Loopback, so select the small
# Windows backend that exposes those APIs without Unix syscalls.
if ! head -n 1 "${SRC_DIR}/tailscale.go" | grep -q 'go:build !windows'; then
  sed -i '1i //go:build !windows\n' "${SRC_DIR}/tailscale.go"
fi
cp "${SCRIPT_DIR}/libtailscale_windows.go" "${SRC_DIR}/tailscale_windows.go"
cp "${SCRIPT_DIR}/libtailscale_windows_forward.c" "${SRC_DIR}/tailscale_windows_forward.c"
sed -i '/#include <sys\/socket.h>/d; /#include <unistd.h>/d' "${SRC_DIR}/tailscale.c"

command -v go >/dev/null || { echo "Go is required to build libtailscale" >&2; exit 1; }
command -v x86_64-w64-mingw32-gcc >/dev/null || {
  echo "x86_64-w64-mingw32-gcc is required to build Windows libtailscale" >&2
  exit 1
}

mkdir -p "${OUT_DIR}"
(
  cd "${SRC_DIR}"
  GOOS=windows GOARCH=amd64 CGO_ENABLED=1 \
    CC="${CC:-x86_64-w64-mingw32-gcc}" \
    go build -buildmode=c-archive -trimpath -o "${OUT_DIR}/libtailscale.a" .
  cp tailscale.h "${OUT_DIR}/tailscale.h"
)

echo "Built ${OUT_DIR}/libtailscale.a"
