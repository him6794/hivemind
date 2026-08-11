#!/usr/bin/env bash
# Builds the managed-proof prover sidecar and stages it for release packaging.
#
# RISC Zero 3.0.6 only supports Linux and macOS prover hosts, so this must run on
# one of those (WSL counts). The resulting binary is copied into
# packaging/managed-prover/, which hivemind-rs/Dockerfile bakes into the worker
# image at /app/prover/.
#
# The guest is built natively (HIVEMIND_ZKVM_USE_DOCKER=0). The Docker builder
# path is deliberately not used here: it needs Docker-in-Docker and, on this
# project's Windows hosts, it repeatedly exhausted the Docker VHD.

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
prover_workspace="$repo_root/zkvm/managed-proof"
staging_dir="$repo_root/packaging/managed-prover"
binary_name="hivemind-managed-proof-prover"

if [[ ! -d "$prover_workspace" ]]; then
  echo "error: prover workspace not found at $prover_workspace" >&2
  exit 66
fi

case "$(uname -s)" in
  Linux | Darwin) ;;
  *)
    echo "error: RISC Zero has no supported prover host on $(uname -s)." >&2
    echo "       Run this on Linux, macOS, or WSL." >&2
    exit 65
    ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is not on PATH" >&2
  exit 69
fi

# risc0-build shells out to the RISC Zero guest toolchain. Without it the guest
# ELF cannot be produced and the build fails deep inside a build script, so
# check up front and say what to install.
if ! rustup toolchain list 2>/dev/null | grep -q '^risc0'; then
  echo "error: the RISC Zero guest Rust toolchain is not installed." >&2
  echo "       Install rzup, then run: rzup install rust" >&2
  echo "       See https://dev.risczero.com/api/zkvm/install" >&2
  exit 69
fi

export HIVEMIND_ZKVM_USE_DOCKER=0

echo "building $binary_name (this compiles the zkVM guest; expect several minutes)"
started_at="$(date +%s)"
(
  cd "$prover_workspace"
  cargo build --release --locked \
    -p hivemind-managed-proof-zkvm \
    --bin "$binary_name"
)
elapsed="$(( $(date +%s) - started_at ))"

built_binary="${CARGO_TARGET_DIR:-$prover_workspace/target}/release/$binary_name"
if [[ ! -x "$built_binary" ]]; then
  echo "error: expected a built prover at $built_binary" >&2
  exit 70
fi

mkdir -p "$staging_dir"
install -m 0755 "$built_binary" "$staging_dir/$binary_name"

echo "built in ${elapsed}s"
echo "staged: $staging_dir/$binary_name"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$staging_dir/$binary_name"
elif command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$staging_dir/$binary_name"
fi

cat <<'NOTE'

Next steps:
  1. Confirm the embedded guest matches the Nodepool trust pin
     (RISC0_MANAGED_GUEST_ID in hivemind-rs/crates/managed-proof/src/lib.rs).
     A mismatch makes the Nodepool reject every envelope this prover produces.
  2. Record the binary SHA-256 and guest image ID in
     docs/zk-managed-proof-build-attestation.md.
  3. Rebuild the worker image so /app/prover/ picks up the staged binary.
NOTE
