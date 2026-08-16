#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SCRIPT_DIR}/.."
VERSION="${LIBTAILSCALE_VERSION:-5e89501def80a6579ca5d0f9a02f336be62b8f2e}"
TARGET_KIND="${LIBTAILSCALE_TARGET:-gnu}"

case "${TARGET_KIND}" in
  gnu)
    OUT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-x86_64}"
    ARTIFACT_NAME="libtailscale.a"
    CC_NAME="${CC:-x86_64-w64-mingw32-gcc}"
    ;;
  msvc)
    OUT_DIR="${LIBTAILSCALE_WINDOWS_DIR:-${ROOT_DIR}/vendor/libtailscale/windows-x86_64-msvc}"
    ARTIFACT_NAME="libtailscale.dll"
    CC_NAME="${CC:-x86_64-w64-mingw32-gcc}"
    ;;
  *)
    echo "LIBTAILSCALE_TARGET must be 'gnu' or 'msvc'" >&2
    exit 1
    ;;
esac

SRC_DIR="${ROOT_DIR}/.cache/libtailscale-${VERSION//\//_}"
if [[ ! -d "${SRC_DIR}/.git" ]]; then
  mkdir -p "${ROOT_DIR}/.cache"
  git clone --depth 1 https://github.com/tailscale/libtailscale.git "${SRC_DIR}"
  git -C "${SRC_DIR}" checkout --detach "${VERSION}"
fi

# The upstream C archive currently uses Unix socketpair/FD-passing code. The Windows client only needs tsnet.Up and tsnet.Loopback, so select the small Windows backend that exposes those APIs without Unix syscalls.
if ! head -n 1 "${SRC_DIR}/tailscale.go" | grep -q 'go:build !windows'; then
  sed -i '1i //go:build !windows\n' "${SRC_DIR}/tailscale.go"
fi
cp "${SCRIPT_DIR}/libtailscale_windows.go" "${SRC_DIR}/tailscale_windows.go"
cp "${SCRIPT_DIR}/libtailscale_windows_forward.c" "${SRC_DIR}/tailscale_windows_forward.c"
sed -i '/#include <sys\/socket.h>/d; /#include <unistd.h>/d' "${SRC_DIR}/tailscale.c"

command -v go >/dev/null || { echo "Go is required to build libtailscale" >&2; exit 1; }
command -v "${CC_NAME}" >/dev/null || {
  echo "C compiler '${CC_NAME}' is required to build the ${TARGET_KIND} Windows libtailscale artifact" >&2
  exit 1
}
mkdir -p "${OUT_DIR}"
(
  cd "${SRC_DIR}"
  if [[ "${TARGET_KIND}" == "gnu" ]]; then
    GOOS=windows GOARCH=amd64 CGO_ENABLED=1 \
      CC="${CC_NAME}" \
      go build -buildmode=c-archive -trimpath -o "${OUT_DIR}/${ARTIFACT_NAME}" .
  else
    temporary_archive="${OUT_DIR}/libtailscale-c-archive.a"
    GOOS=windows GOARCH=amd64 CGO_ENABLED=1 \
      CC="${CC_NAME}" \
      go build -buildmode=c-archive -trimpath -o "${temporary_archive}" .
    "${CC_NAME}" -shared -o "${OUT_DIR}/${ARTIFACT_NAME}" \
      -Wl,--whole-archive "${temporary_archive}" -Wl,--no-whole-archive \
      -Wl,--export-all-symbols -Wl,--out-implib,"${OUT_DIR}/libtailscale.dll.a" \
      -lws2_32 -ladvapi32
    rm -f "${temporary_archive}" "${OUT_DIR}/libtailscale.dll.a"
  fi
  cp tailscale.h "${OUT_DIR}/tailscale.h"
)

LIBTAILSCALE_TARGET="${TARGET_KIND}" \
LIBTAILSCALE_WINDOWS_DIR="${OUT_DIR}" \
  bash "${SCRIPT_DIR}/validate_libtailscale_windows.sh"

echo "Built ${TARGET_KIND} artifact ${OUT_DIR}/${ARCHIVE_NAME} from libtailscale ${VERSION}"
