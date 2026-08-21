#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: deploy-worker-windows-arm64.sh PACKAGE_DIR DEST_DIR

Deploy a previously built Windows ARM64 worker package without downloading or
regenerating binaries. The package must contain hivemind-bin.exe,
libtailscale.dll, and the matching vcruntime140.dll.
EOF
  exit 2
}

[[ $# -eq 2 ]] || usage
PACKAGE_DIR="$(cd "$1" 2>/dev/null && pwd)" || {
  echo "package directory does not exist: $1" >&2
  exit 1
}
DEST_DIR="$2"

if command -v llvm-readobj >/dev/null 2>&1; then
  PE_HEADER_TOOL="llvm-readobj"
  PE_HEADER_TOOL_KIND="readobj"
elif command -v llvm-objdump >/dev/null 2>&1; then
  PE_HEADER_TOOL="llvm-objdump"
  PE_HEADER_TOOL_KIND="objdump"
elif command -v aarch64-w64-mingw32-objdump >/dev/null 2>&1; then
  PE_HEADER_TOOL="aarch64-w64-mingw32-objdump"
  PE_HEADER_TOOL_KIND="objdump"
elif command -v objdump >/dev/null 2>&1; then
  PE_HEADER_TOOL="objdump"
  PE_HEADER_TOOL_KIND="objdump"
else
  echo "llvm-readobj, llvm-objdump, aarch64-w64-mingw32-objdump, or objdump is required to verify PE machine types" >&2
  exit 1
fi

if command -v llvm-objdump >/dev/null 2>&1; then
  PE_IMPORT_TOOL="llvm-objdump"
elif command -v aarch64-w64-mingw32-objdump >/dev/null 2>&1; then
  PE_IMPORT_TOOL="aarch64-w64-mingw32-objdump"
elif command -v objdump >/dev/null 2>&1; then
  PE_IMPORT_TOOL="objdump"
else
  PE_IMPORT_TOOL=""
fi

binary="${PACKAGE_DIR}/hivemind-bin.exe"
dll="${PACKAGE_DIR}/libtailscale.dll"
runtime="${PACKAGE_DIR}/vcruntime140.dll"
[[ -f "$binary" ]] || { echo "missing ARM64 worker executable: $binary" >&2; exit 1; }
[[ -f "$dll" ]] || { echo "missing ARM64 libtailscale DLL: $dll" >&2; exit 1; }
[[ -f "$runtime" ]] || { echo "missing ARM64 Visual C++ runtime DLL: $runtime" >&2; exit 1; }

assert_arm64() {
  local path="$1"
  local header
  if [[ "$PE_HEADER_TOOL_KIND" == "readobj" ]]; then
    header="$($PE_HEADER_TOOL --file-headers "$path")"
    if ! grep -Eiq 'IMAGE_FILE_MACHINE_ARM64|Arch: aarch64' <<<"$header"; then
      echo "refusing non-ARM64 PE artifact: $path" >&2
      printf '%s\n' "$header" >&2
      exit 1
    fi
    return
  fi

  header="$($PE_HEADER_TOOL -f "$path")"
  if ! grep -Eiq 'architecture: (aarch64|arm64)|coff-arm64|pe-arm-wince|[[:space:]](ARM64|AArch64)[[:space:]]' <<<"$header"; then
    echo "refusing non-ARM64 PE artifact: $path" >&2
    printf '%s\n' "$header" >&2
    exit 1
  fi
}

assert_arm64 "$binary"
assert_arm64 "$dll"
assert_arm64 "$runtime"

if [[ -n "$PE_IMPORT_TOOL" ]]; then
  imports="$($PE_IMPORT_TOOL -p "$dll")"
elif [[ "$PE_HEADER_TOOL_KIND" == "readobj" ]]; then
  imports="$($PE_HEADER_TOOL --coff-imports "$dll")"
else
  echo "an import-table inspection tool is required to validate libtailscale.dll dependencies" >&2
  exit 1
fi
if grep -Eiq 'DLL Name: (libgcc|libwinpthread|libmingwex|msys)' <<<"$imports"; then
  echo "refusing DLL with undeployed MinGW runtime dependency: $dll" >&2
  exit 1
fi

sha_file="${PACKAGE_DIR}/SHA256SUMS"
[[ -f "$sha_file" ]] || {
  echo "missing package integrity manifest: $sha_file" >&2
  exit 1
}
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$PACKAGE_DIR" && tr -d '\r' < SHA256SUMS | sha256sum -c -)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$PACKAGE_DIR" && tr -d '\r' < SHA256SUMS | shasum -a 256 -c -)
else
  echo "sha256sum or shasum is required to verify package integrity" >&2
  exit 1
fi
for required in hivemind-bin.exe libtailscale.dll vcruntime140.dll; do
  grep -Eiq "[[:xdigit:]]{64}[[:space:]]+\\*${required}[[:space:]]*$" "$sha_file" || {
    echo "package integrity manifest does not cover ${required}: $sha_file" >&2
    exit 1
  }
done

mkdir -p "$DEST_DIR"
DEST_DIR="$(cd "$DEST_DIR" && pwd)"
cp -f "$binary" "$DEST_DIR/hivemind-bin.exe"
cp -f "$dll" "$DEST_DIR/libtailscale.dll"
cp -f "$runtime" "$DEST_DIR/vcruntime140.dll"
for file in .env.worker.example README.md start-worker.ps1 native-dependency-provenance.json SHA256SUMS manifest.json; do
  if [[ -f "${PACKAGE_DIR}/${file}" ]]; then
    cp -f "${PACKAGE_DIR}/${file}" "${DEST_DIR}/${file}"
  fi
done

if [[ ! -f "$DEST_DIR/.env.worker" && -f "$DEST_DIR/.env.worker.example" ]]; then
  cp "$DEST_DIR/.env.worker.example" "$DEST_DIR/.env.worker"
  echo "Created $DEST_DIR/.env.worker from the package template; configure it before starting the worker." >&2
fi

cat <<EOF
Deployed verified Windows ARM64 worker package to:
  ${DEST_DIR}
Start it on Windows ARM64 with:
  PowerShell -ExecutionPolicy Bypass -File .\\start-worker.ps1
EOF
