$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "build-managed-prover.sh"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "build-managed-prover.sh must exist."
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw

foreach ($expected in @(
    "Linux | Darwin",
    "MINGW",
    "WSL",
    "RECURSION_SRC_PATH",
    "recursion_zkr.zip",
    "744b999f0a35b3c86753311c7efb2a0054be21727095cf105af6ee7d3f4d8849",
    "resolve_recursion_artifact",
    "official upstream offline escape hatch"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "managed prover build script must implement the supported-host contract via '$expected'."
    }
}

if (!$scriptText.Contains("native Windows")) {
    throw "managed prover build script must explicitly reject native Windows proving."
}

Write-Host "managed prover build contract tests passed"
