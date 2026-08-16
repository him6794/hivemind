#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SCRIPT_DIR}/.."
TARGET_KIND="${LIBTAILSCALE_TARGET:-gnu}"

case "${TARGET_KIND}" in
  gnu)
    ARTIFACT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-x86_64}"
    ARCHIVE_NAME="libtailscale.a"
    ;;
  msvc)
    ARTIFACT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-x86_64-msvc}"
    ARCHIVE_NAME="libtailscale.dll"
    ;;
  *)
    echo "LIBTAILSCALE_TARGET must be 'gnu' or 'msvc'" >&2
    exit 1
    ;;
esac

ARCHIVE="${ARTIFACT_DIR}/${ARCHIVE_NAME}"
HEADER="${ARTIFACT_DIR}/tailscale.h"
[[ -f "${ARCHIVE}" ]] || { echo "missing archive: ${ARCHIVE}" >&2; exit 1; }
[[ -f "${HEADER}" ]] || { echo "missing header: ${HEADER}" >&2; exit 1; }

if command -v llvm-nm >/dev/null; then
  SYMBOL_TOOL=(llvm-nm)
elif command -v nm >/dev/null; then
  SYMBOL_TOOL=(nm)
else
  echo "llvm-nm or nm is required to inspect ${ARCHIVE}" >&2
  exit 1
fi

SYMBOLS="$(${SYMBOL_TOOL[@]} "${ARCHIVE}")"
for symbol in tailscale_new tailscale_set_dir tailscale_set_hostname tailscale_set_authkey \
  tailscale_set_control_url tailscale_up tailscale_close tailscale_loopback \
  tailscale_getips tailscale_listen_forward tailscale_errmsg; do
  if ! grep -E "(^|[[:space:]])_?${symbol}([[:space:]]|$)" <<<"${SYMBOLS}" >/dev/null; then
    echo "archive is missing required symbol ${symbol}: ${ARCHIVE}" >&2
    exit 1
  fi
done

if [[ "${TARGET_KIND}" == "msvc" ]]; then
  if ! command -v objdump >/dev/null; then
    echo "objdump is required to inspect MSVC DLL dependencies" >&2
    exit 1
  fi
  dependencies="$(objdump -p "${ARCHIVE}")"
  if grep -Ei 'DLL Name: (libgcc|libwinpthread|libmingwex|msys)' <<<"${dependencies}" >/dev/null; then
    echo "MSVC DLL has an undeployed MinGW runtime dependency: ${ARCHIVE}" >&2
    exit 1
  fi
fi

if command -v llvm-readobj >/dev/null; then
  llvm-readobj --file-headers "${ARCHIVE}" >/dev/null
elif command -v objdump >/dev/null; then
  objdump -f "${ARCHIVE}" >/dev/null
fi

echo "Validated ${TARGET_KIND} libtailscale artifact: ${ARCHIVE}"
