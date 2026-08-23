#!/usr/bin/env bash
# Verifies that a staged managed-proof prover sidecar is the tracked, pinned
# build before it is baked into a release image.
#
# The sidecar cannot be rebuilt inside Docker: the guest image ID depends on
# the exact risc0 guest toolchain that produced the Nodepool trust pin, and a
# container-built toolchain produces a different guest whose every proof the
# Nodepool rejects. docs/zk-managed-proof-build-attestation.md records the full
# history of that failure mode.
#
# This script therefore checks what CAN be checked about an already-built
# binary without re-running the prover:
#
#   1. The file exists and is a Linux x86_64 ELF executable.
#   2. Its SHA-256 matches the digest recorded in
#      docs/zk-managed-proof-build-attestation.md, so any rebuild that was not
#      re-attested fails here instead of shipping silently.
#
# A digest mismatch means either a stale staging directory or an unattested
# rebuild; both must go through scripts/build-managed-prover.sh plus a new
# attestation entry before a release image may include them.

set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
default_sidecar="$repo_root/packaging/managed-prover/hivemind-managed-proof-prover"
attestation="$repo_root/docs/zk-managed-proof-build-attestation.md"

sidecar="${1:-$default_sidecar}"

if [[ ! -f "$sidecar" ]]; then
  echo "error: staged prover sidecar not found at $sidecar" >&2
  echo "       Build it with scripts/build-managed-prover.sh on a supported host first." >&2
  exit 66
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual_sha256="$(sha256sum "$sidecar" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual_sha256="$(shasum -a 256 "$sidecar" | awk '{print $1}')"
else
  echo "error: neither sha256sum nor shasum is available to verify the sidecar" >&2
  exit 69
fi

file_output="$(file -b "$sidecar" 2>/dev/null || true)"
case "$file_output" in
  *"ELF 64-bit"*"x86-64"*)
    ;;
  *)
    echo "error: staged prover is not a Linux x86_64 executable: $file_output" >&2
    echo "       RISC Zero proving hosts are Linux x86_64 only for this deployment." >&2
    exit 65
    ;;
esac

attested_sha256="$(grep -oE '^[0-9a-f]{64}  hivemind-managed-proof-prover' "$attestation" | awk '{print $1}' | tail -1 || true)"

echo "staged sidecar: $sidecar"
echo "sha256:         $actual_sha256"

status=0
if [[ -z "$attested_sha256" ]]; then
  echo "error: no attested hivemind-managed-proof-prover digest found in" >&2
  echo "       docs/zk-managed-proof-build-attestation.md; record this build there first." >&2
  status=71
elif [[ "$actual_sha256" != "$attested_sha256" ]]; then
  echo "error: staged sidecar does not match the attested build." >&2
  echo "       attested: $attested_sha256" >&2
  echo "       staged:   $actual_sha256" >&2
  echo "       Rebuild with scripts/build-managed-prover.sh and update the attestation," >&2
  echo "       or re-stage the attested artifact." >&2
  status=71
else
  echo "OK: staged sidecar matches the attested pinned-guest build."
fi

exit "$status"
