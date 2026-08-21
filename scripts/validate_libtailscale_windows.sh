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
    EXPECTED_MACHINE="x86-64"
    ;;
  arm64-msvc)
    ARTIFACT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-aarch64-msvc}"
    ARCHIVE_NAME="libtailscale.dll"
    EXPECTED_MACHINE="ARM64"
    ;;
  *)
    echo "LIBTAILSCALE_TARGET must be 'gnu', 'msvc', or 'arm64-msvc'" >&2
    exit 1
    ;;
esac

ARCHIVE="${ARTIFACT_DIR}/${ARCHIVE_NAME}"
HEADER="${ARTIFACT_DIR}/tailscale.h"
[[ -f "${ARCHIVE}" ]] || { echo "missing archive: ${ARCHIVE}" >&2; exit 1; }
[[ -f "${HEADER}" ]] || { echo "missing header: ${HEADER}" >&2; exit 1; }

if command -v llvm-nm >/dev/null; then
  SYMBOL_TOOL=(llvm-nm)
elif command -v aarch64-w64-mingw32-nm >/dev/null; then
  SYMBOL_TOOL=(aarch64-w64-mingw32-nm)
elif command -v x86_64-w64-mingw32-nm >/dev/null; then
  SYMBOL_TOOL=(x86_64-w64-mingw32-nm)
elif command -v nm >/dev/null; then
  SYMBOL_TOOL=(nm)
else
  echo "llvm-nm, a target-prefixed nm, or nm is required to inspect ${ARCHIVE}" >&2
  exit 1
fi

SYMBOLS="$("${SYMBOL_TOOL[@]}" "${ARCHIVE}")"
for symbol in tailscale_new tailscale_set_dir tailscale_set_hostname tailscale_set_authkey \
  tailscale_set_control_url tailscale_up tailscale_close tailscale_loopback \
  tailscale_getips tailscale_listen_forward tailscale_errmsg; do
  if ! grep -E "(^|[[:space:]])_?${symbol}([[:space:]]|$)" <<<"${SYMBOLS}" >/dev/null; then
    echo "archive is missing required symbol ${symbol}: ${ARCHIVE}" >&2
    exit 1
  fi
done

if [[ "${TARGET_KIND}" == "msvc" || "${TARGET_KIND}" == "arm64-msvc" ]]; then
  if command -v llvm-objdump >/dev/null; then
    OBJDUMP_TOOL=(llvm-objdump)
  elif [[ "${TARGET_KIND}" == "arm64-msvc" ]] && command -v aarch64-w64-mingw32-objdump >/dev/null; then
    OBJDUMP_TOOL=(aarch64-w64-mingw32-objdump)
  elif command -v objdump >/dev/null; then
    OBJDUMP_TOOL=(objdump)
  else
    echo "llvm-objdump, a target-prefixed objdump, or objdump is required to inspect Windows DLL dependencies" >&2
    exit 1
  fi

  dependencies="$("${OBJDUMP_TOOL[@]}" -p "${ARCHIVE}")"
  if grep -Ei 'DLL Name: (libgcc|libwinpthread|libmingwex|msys)' <<<"${dependencies}" >/dev/null; then
    echo "Windows DLL has an undeployed MinGW runtime dependency: ${ARCHIVE}" >&2
    exit 1
  fi
  if command -v llvm-readobj >/dev/null; then
    headers="$(llvm-readobj --file-headers "${ARCHIVE}")"
    if [[ "${EXPECTED_MACHINE}" == "ARM64" ]]; then
      machine_pattern='IMAGE_FILE_MACHINE_ARM64|Arch: aarch64'
    else
      machine_pattern='IMAGE_FILE_MACHINE_AMD64|Arch: x86-64'
    fi
  else
    headers="$("${OBJDUMP_TOOL[@]}" -f "${ARCHIVE}")"
    machine_pattern="${EXPECTED_MACHINE}|pe-${EXPECTED_MACHINE}"
  fi
  if ! grep -Eiq "${machine_pattern}" <<<"${headers}"; then
    echo "Windows DLL machine type does not match ${EXPECTED_MACHINE}: ${ARCHIVE}" >&2
    exit 1
  fi
fi

if command -v llvm-readobj >/dev/null; then
  llvm-readobj --file-headers "${ARCHIVE}" >/dev/null
elif command -v llvm-objdump >/dev/null; then
  llvm-objdump -f "${ARCHIVE}" >/dev/null
elif command -v objdump >/dev/null; then
  objdump -f "${ARCHIVE}" >/dev/null
fi

echo "Validated ${TARGET_KIND} libtailscale artifact: ${ARCHIVE}"
