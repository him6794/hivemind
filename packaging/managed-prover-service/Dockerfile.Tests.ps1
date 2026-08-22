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

Write-Host "managed-prover provider Dockerfile contract tests passed"
