#!/usr/bin/env bash
# Builds the managed-proof prover sidecar and stages it for release packaging.
#
# RISC Zero 3.0.6 only supports Linux and macOS prover hosts, so this must run on
# one of those (WSL counts). The resulting binary is copied into
# packaging/managed-prover/, which hivemind-rs/Dockerfile bakes into the worker
# image at /app/prover/.
#
# On a host that cannot reach risc0-artifacts.s3.us-west-2.amazonaws.com, the
# risc0-circuit-recursion build script fails after three download attempts. Set
# RECURSION_SRC_PATH to a local copy of recursion_zkr.zip; that is the official upstream offline escape hatch.
# The build script verifies its SHA-256 before use, so this reuses a checked
# artifact rather than skipping the check.
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

host_os="$(uname -s)"
case "$host_os" in
  Linux | Darwin)
    # WSL intentionally reports Linux here and follows the supported path.
    ;;
  MINGW* | MSYS* | CYGWIN*)
    echo "error: native Windows proving is unsupported by RISC Zero 3.0.6." >&2
    echo "       Run this script inside WSL, Linux, or macOS." >&2
    exit 65
    ;;
  *)
    echo "error: RISC Zero has no supported prover host on $host_os." >&2
    echo "       Run this on Linux, macOS, or WSL." >&2
    exit 65
    ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is not on PATH" >&2
  exit 69
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc is not on PATH" >&2
  exit 69
fi
if ! rustc --version | grep -Eq '^rustc 1\.90\.0([[:space:]]|$)'; then
  echo "error: the host Rust compiler must be exactly 1.90.0." >&2
  echo "       Refusing to build a prover with an unverified host compiler." >&2
  exit 69
fi

# risc0-build shells out to the RISC Zero guest toolchain. Without it the guest
# ELF cannot be produced and the build fails deep inside a build script, so
# check up front and say what to install.
if ! cargo +risc0 --version >/dev/null 2>&1; then
  echo "error: the RISC Zero guest Rust toolchain is not installed." >&2
  echo "       Install rzup, then run: rzup install rust 1.97.0" >&2
  echo "       See https://dev.risczero.com/api/zkvm/install" >&2
  exit 69
fi
if ! cargo +risc0 --version | grep -Eq '1\.97\.0'; then
  echo "error: the RISC Zero guest Rust toolchain must be exactly 1.97.0." >&2
  echo "       Refusing to build a prover with an unverified guest compiler." >&2
  exit 69
fi

export HIVEMIND_ZKVM_USE_DOCKER=0
export RISC0_BUILD_LOCKED=1
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"

# RISC Zero includes source locations in the guest ELF (and therefore in the
# image ID). The trusted v0 guest was built from the canonical WSL checkout;
# remap both frozen v0 crate sources and dependency registry paths so a
# supported proving host reproduces that exact guest rather than merely the
# same source text. The specific v0 mappings must precede the repository-wide
# mapping for toolchains that apply remaps in declaration order.
canonical_guest_source_root="/run/desktop/mnt/host/d/hivemind"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cargo_registry_index="index.crates.io-1949cf8c6b5b557f"
path_remap_flags=(
  "--remap-path-prefix=$repo_root/executor-rs/crates/managed-function-runtime-v0/src/lib.rs=$canonical_guest_source_root/executor-rs/crates/managed-function-runtime/src/lib.rs"
  "--remap-path-prefix=$repo_root/hivemind-rs/crates/managed-proof-v0/src/lib.rs=$canonical_guest_source_root/hivemind-rs/crates/managed-proof/src/lib.rs"
  "--remap-path-prefix=$repo_root=$canonical_guest_source_root"
  "--remap-path-prefix=$cargo_home/registry/src/$cargo_registry_index/no_std_strings-0.1.3=/home/remi/.cargo/registry/src/$cargo_registry_index/no_std_strings-0.1.3"
  "--remap-path-prefix=$cargo_home=/root/.cargo"
)
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }${path_remap_flags[*]}"

# RISC Zero's recursion circuit build downloads this artifact from its public
# bucket. The upstream build script supports RECURSION_SRC_PATH as the official
# upstream offline escape hatch; keep that contract intact and verify the artifact
# before handing it to Cargo. This is useful on WSL/macOS hosts whose network
# policy cannot reach the bucket, and avoids patching anything in the registry.
recursion_artifact_sha256="744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849"

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$path" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$path" | awk '{print $1}'
  else
    echo "error: neither sha256sum nor shasum is available to verify recursion_zkr.zip" >&2
    return 69
  fi
}

absolute_path() {
  local path="$1"
  local directory
  directory="$(cd "$(dirname "$path")" && pwd)"
  printf '%s/%s\n' "$directory" "$(basename "$path")"
}

resolve_recursion_artifact() {
  local artifact="${RECURSION_SRC_PATH:-}"

  if [[ -n "$artifact" ]]; then
    if [[ ! -f "$artifact" ]]; then
      echo "error: RECURSION_SRC_PATH does not point to a file: $artifact" >&2
      return 66
    fi
    artifact="$(absolute_path "$artifact")"
  else
    local target_root="${CARGO_TARGET_DIR:-$prover_workspace/target}"
    local discovered
    discovered="$(find "$target_root" -type f -name recursion_zkr.zip -print -quit 2>/dev/null || true)"
    if [[ -n "$discovered" ]]; then
      artifact="$(absolute_path "$discovered")"
      export RECURSION_SRC_PATH="$artifact"
      echo "found cached recursion artifact at $artifact"
    fi
  fi

  if [[ -z "$artifact" ]]; then
    echo "no local recursion_zkr.zip found; RISC Zero will try its normal network download" >&2
    echo "       Set RECURSION_SRC_PATH=/path/to/recursion_zkr.zip for the official upstream offline escape hatch." >&2
    return 0
  fi

  local actual_sha256
  actual_sha256="$(sha256_file "$artifact")" || return $?
  if [[ "$actual_sha256" != "$recursion_artifact_sha256" ]]; then
    echo "error: recursion artifact SHA-256 mismatch for $artifact" >&2
    echo "       expected: $recursion_artifact_sha256" >&2
    echo "       actual:   $actual_sha256" >&2
    return 65
  fi

  export RECURSION_SRC_PATH="$artifact"
  echo "using verified recursion artifact: $artifact"
}

resolve_recursion_artifact

# The guest image ID depends on the exact guest toolchain and build environment,
# not just on the tracked source. A prover built somewhere else embeds a
# different guest, and the Nodepool then rejects every proof it produces —
# managed tasks fail closed with no clue as to why. Refuse to stage such a
# binary: this is the same equality the Nodepool enforces at settlement time.
echo "checking that this environment reproduces the trusted guest image ID"
(
  cd "$prover_workspace"
  output="$(cargo test --locked -p hivemind-managed-proof-zkvm \
    tests::generated_guest_id_matches_nodepool_trust_pin -- --exact --nocapture 2>&1 | tee /dev/stderr)"
  grep -Eq 'test tests::generated_guest_id_matches_nodepool_trust_pin[[:space:]].*ok' <<<"$output"
) || {
  echo "error: this environment does not reproduce the pinned guest image ID." >&2
  echo "       A prover built here would have every proof rejected." >&2
  echo "       Either build where the pin was produced, or re-pin deliberately" >&2
  echo "       and regenerate docs/zk-managed-proof-build-attestation.md." >&2
  exit 71
}

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

The staged prover embeds the pinned guest: that was checked before building.

Next steps:
  1. Record the binary SHA-256 in docs/zk-managed-proof-build-attestation.md.
  2. Rebuild the worker image so /app/prover/ picks up the staged binary.
  3. Run one managed task end to end. A guest mismatch that somehow survives the
     check above shows up as every managed task failing with
     "Managed proof verification failed" in the admin audit log.
NOTE
