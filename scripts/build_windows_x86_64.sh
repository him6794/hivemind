#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}/../hivemind-rs"

rustc --version
cargo --version

if ! command -v x86_64-w64-mingw32-dlltool >/dev/null || \
   ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
    gcc-mingw-w64-x86-64 \
    g++-mingw-w64-x86-64 \
    binutils-mingw-w64-x86-64 \
    protobuf-compiler \
    pkg-config \
    nasm \
    cmake \
    git \
    curl \
    ca-certificates >/tmp/apt.log 2>&1
fi

rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}"

export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${SCRIPT_DIR}/../hivemind-rs/target-windows}"
TAILSCALE_DIR="${TAILSCALE_WINDOWS_DIR:-${SCRIPT_DIR}/../vendor/tailscale/windows-x86_64}"
LIBTAILSCALE_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${SCRIPT_DIR}/../vendor/libtailscale/windows-x86_64}"

if [[ ! -f "${LIBTAILSCALE_DIR}/libtailscale.a" ]]; then
  LIBTAILSCALE_WINDOWS_DIR="${LIBTAILSCALE_DIR}" "${SCRIPT_DIR}/fetch_libtailscale_windows.sh"
fi

cargo build --release --no-default-features --features master --bin hivemind-master --target x86_64-pc-windows-gnu
cargo build --release --no-default-features --features worker --bin hivemind-worker --target x86_64-pc-windows-gnu

# monty is the managed-function runtime used by the worker. It must be built
# for Windows; renaming the host Linux binary to monty.exe produces an ELF
# file that Windows cannot execute.
(cd "${SCRIPT_DIR}/../executor-rs" && \
  CARGO_TARGET_DIR="${CARGO_TARGET_DIR}" \
  cargo build --release --package monty-cli --bin monty --target x86_64-pc-windows-gnu)

# Create release package directories with UI assets
RELEASE_DIR="${SCRIPT_DIR}/../dist/windows"
MASTER_DIR="${RELEASE_DIR}/master"
WORKER_DIR="${RELEASE_DIR}/worker"

if [[ ! -f "${TAILSCALE_DIR}/tailscale.exe" || ! -f "${TAILSCALE_DIR}/tailscaled.exe" ]]; then
  TAILSCALE_WINDOWS_DIR="${TAILSCALE_DIR}" "${SCRIPT_DIR}/fetch_tailscale_windows.sh"
fi

rm -rf "${MASTER_DIR}" "${WORKER_DIR}"
mkdir -p "${MASTER_DIR}" "${WORKER_DIR}"
mkdir -p "${MASTER_DIR}/vpn" "${WORKER_DIR}/vpn"

# Copy binaries
cp "${CARGO_TARGET_DIR}/x86_64-pc-windows-gnu/release/hivemind-master.exe" "${MASTER_DIR}/"
cp "${CARGO_TARGET_DIR}/x86_64-pc-windows-gnu/release/hivemind-worker.exe" "${WORKER_DIR}/"
cp "${TAILSCALE_DIR}/tailscale.exe" "${MASTER_DIR}/vpn/"
cp "${TAILSCALE_DIR}/tailscaled.exe" "${MASTER_DIR}/vpn/"
cp "${TAILSCALE_DIR}/tailscale.exe" "${WORKER_DIR}/vpn/"
cp "${TAILSCALE_DIR}/tailscaled.exe" "${WORKER_DIR}/vpn/"

# Copy UI assets - master-ui
cp -r "${SCRIPT_DIR}/../frontend/master-ui/dist" "${MASTER_DIR}/ui"
# Copy UI assets - worker-ui
cp -r "${SCRIPT_DIR}/../frontend/worker-ui/dist" "${WORKER_DIR}/ui"

# Copy Windows monty (the managed function runtime executor)
cp "${CARGO_TARGET_DIR}/x86_64-pc-windows-gnu/release/monty.exe" "${MASTER_DIR}/monty.exe"
cp "${CARGO_TARGET_DIR}/x86_64-pc-windows-gnu/release/monty.exe" "${WORKER_DIR}/monty.exe"

ls -lh "${MASTER_DIR}/"
ls -lh "${WORKER_DIR}/"

echo ""
echo "Release packages ready:"
echo "  ${MASTER_DIR}/"
echo "  ${WORKER_DIR}/"
