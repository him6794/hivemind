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
    "official upstream offline escape hatch",
    "rustc --version",
    "1\.90\.0",
    "cargo +risc0 --version",
    "1\.97\.0",
    "RISC0_BUILD_LOCKED=1",
    "canonical_guest_source_root",
    "managed-function-runtime-v0/src/lib.rs",
    "--remap-path-prefix=",
    "no_std_strings-0.1.3",
    "tests::generated_guest_id_matches_nodepool_trust_pin"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "managed prover build script must implement the supported-host contract via '$expected'."
    }
}

if ($scriptText.Contains("managed-function-runtime/src/v0.rs")) {
    throw "managed prover build script must remap the independent frozen v0 crate, not a removed source alias."
}

if (!$scriptText.Contains("native Windows")) {
    throw "managed prover build script must explicitly reject native Windows proving."
}

Write-Host "managed prover build contract tests passed"
