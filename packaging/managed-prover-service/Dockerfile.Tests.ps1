$ErrorActionPreference = "Stop"

$dockerfile = Join-Path $PSScriptRoot "Dockerfile"
if (!(Test-Path -LiteralPath $dockerfile)) {
    throw "managed-prover provider Dockerfile must exist."
}

$dockerText = Get-Content -LiteralPath $dockerfile -Raw
foreach ($expected in @(
    "linux/amd64",
    "hivemind-managed-prover-service",
    "hivemind-managed-proof-prover",
    "MANAGED_PROOF_AUTH_PUBLIC_KEY_PEM",
    "MANAGED_PROVER_TLS_CLIENT_CA_PATH",
    "debian:trixie-slim",
    "USER 10001:10001"
)) {
    if (!$dockerText.Contains($expected)) {
        throw "provider Dockerfile must contain '$expected'."
    }
}

# Reproducibility contract: the pinned sidecar cannot be built inside the
# image (the guest image ID follows the risc0 toolchain, not the source), so
# the Dockerfile must say so and point operators at the staged-build workflow.
foreach ($expected in @(
    "docs/zk-managed-proof-build-attestation.md",
    "scripts/build-managed-prover.sh"
)) {
    if (!$dockerText.Contains($expected)) {
        throw "provider Dockerfile must reference '$expected' in its reproducibility note."
    }
}

# The sidecar COPY is a plain path, which makes BuildKit fail a fresh checkout
# with an explicit missing-file error instead of silently shipping a provider
# without its prover. Guard against that being weakened into an optional copy.
if ($dockerText -match 'COPY\s+\S*\$\{?[A-Za-z_]+\}?.*hivemind-managed-proof-prover') {
    throw "provider Dockerfile sidecar COPY must stay a literal path so a missing staged binary fails the build."
}

if (!$dockerText.Contains("COPY packaging/managed-prover/hivemind-managed-proof-prover /app/prover/hivemind-managed-proof-prover")) {
    throw "provider Dockerfile must copy the staged sidecar from packaging/managed-prover."
}

# The verification script is the release-artifact gate; it must exist and know
# how to compare a staged binary against the attested digest.
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$verifyScript = Join-Path $repoRoot "scripts/verify-staged-prover.sh"
if (!(Test-Path -LiteralPath $verifyScript)) {
    throw "scripts/verify-staged-prover.sh must exist to gate release images on the attested digest."
}
$verifyText = Get-Content -LiteralPath $verifyScript -Raw -Encoding UTF8
foreach ($expected in @(
    "zk-managed-proof-build-attestation.md",
    "sha256sum",
    "ELF 64-bit",
    "build-managed-prover.sh"
)) {
    if (!$verifyText.Contains($expected)) {
        throw "verify-staged-prover.sh must check the staged sidecar via '$expected'."
    }
}

Write-Host "managed-prover provider Dockerfile contract tests passed"
