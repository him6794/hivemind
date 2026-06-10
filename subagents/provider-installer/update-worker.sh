#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="/opt/hivemind-worker"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    *) echo "Unknown argument: $1" >&2; exit 1 ;;
  esac
done

release_public_key() {
  local release_dir="$1"
  if [[ -n "${HIVEMIND_RELEASE_PUBLIC_KEY:-}" ]]; then
    if [[ -f "$HIVEMIND_RELEASE_PUBLIC_KEY" ]]; then
      printf '%s\n' "$HIVEMIND_RELEASE_PUBLIC_KEY"
      return 0
    fi
    echo "HIVEMIND_RELEASE_PUBLIC_KEY points to a missing file: $HIVEMIND_RELEASE_PUBLIC_KEY" >&2
    return 1
  fi

  local script_dir install_root
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  install_root="$(dirname "$release_dir")"
  if [[ -f "$script_dir/release-public-key.pem" ]]; then
    printf '%s\n' "$script_dir/release-public-key.pem"
    return 0
  fi
  if [[ -f "$install_root/release-public-key.pem" ]]; then
    printf '%s\n' "$install_root/release-public-key.pem"
    return 0
  fi

  echo "Missing trusted release public key. Set HIVEMIND_RELEASE_PUBLIC_KEY or place release-public-key.pem next to this script or install root. Do not trust keys from release/." >&2
  return 1
}

verify_signed_sums() {
  local release_dir="$1"
  local sums="$release_dir/SHA256SUMS"
  local signature="$release_dir/SHA256SUMS.sig"
  if [[ ! -f "$sums" ]]; then
    echo "Missing signed checksum manifest: $sums" >&2
    return 1
  fi
  if [[ ! -f "$signature" ]]; then
    echo "Missing checksum manifest signature: $signature" >&2
    return 1
  fi
  if ! command -v openssl >/dev/null 2>&1; then
    echo "OpenSSL is required to verify release signatures." >&2
    return 1
  fi
  local public_key
  public_key="$(release_public_key "$release_dir")"
  openssl dgst -sha256 -verify "$public_key" -signature "$signature" "$sums" >/dev/null
}

expected_sha256() {
  local artifact="$1"
  local release_dir="$2"
  local sums="$release_dir/SHA256SUMS"
  local name
  name="$(basename "$artifact")"

  local hash entry base
  while read -r hash entry; do
    base="$(basename "${entry#\*}")"
    if [[ "$hash" =~ ^[A-Fa-f0-9]{64}$ && "$base" == "$name" ]]; then
      printf '%s\n' "${hash,,}"
      return 0
    fi
  done < "$sums"
  echo "Signed SHA256SUMS does not contain an entry for $name" >&2
  return 1
}

copy_verified_artifact() {
  local src="$1"
  local dst="$2"
  local release_dir="$3"

  if [[ ! -f "$src" ]]; then
    echo "Missing release artifact: $src" >&2
    return 1
  fi
  verify_signed_sums "$release_dir"
  local expected actual copied
  expected="$(expected_sha256 "$src" "$release_dir")"
  actual="$(sha256sum "$src" | awk '{print tolower($1)}')"
  if [[ "$actual" != "$expected" ]]; then
    echo "Checksum mismatch for $src. Expected $expected but found $actual." >&2
    return 1
  fi
  cp "$src" "$dst"
  copied="$(sha256sum "$dst" | awk '{print tolower($1)}')"
  if [[ "$copied" != "$expected" ]]; then
    echo "Copied artifact checksum mismatch for $dst." >&2
    return 1
  fi
}

BIN_DIR="$INSTALL_DIR/bin"
RELEASE_DIR="$INSTALL_DIR/release"
SRC="$RELEASE_DIR/worker-executor"
DST="$BIN_DIR/worker-executor"

mkdir -p "$BIN_DIR"
copy_verified_artifact "$SRC" "$DST" "$RELEASE_DIR"
chmod +x "$DST"

if [[ -f "$RELEASE_DIR/version.txt" ]]; then
  echo "Updated worker to version $(tr -d '\r\n' < "$RELEASE_DIR/version.txt")"
else
  echo "Updated worker binary (version metadata missing)"
fi
