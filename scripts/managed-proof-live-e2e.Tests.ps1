$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "managed-proof-live-e2e.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "managed-proof-live-e2e.ps1 must exist."
}

# Read as UTF-8 explicitly: the harness contains no non-ASCII text, but the
# repo convention keeps every PowerShell contract test decoding-safe.
$scriptText = Get-Content -LiteralPath $scriptPath -Raw -Encoding UTF8

function Assert-Contains {
    param(
        [Parameter(Mandatory = $true)][string]$Needle,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (!$scriptText.Contains($Needle)) {
        throw $Message
    }
}

function Assert-NotContains {
    param(
        [Parameter(Mandatory = $true)][string]$Pattern,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if ($scriptText -match $Pattern) {
        throw $Message
    }
}

# The protected-enrollment flow phases, in order.
foreach ($expected in @(
        "PHASE 1 website login",
        "PHASE 2 enrollment credential issue/redeem",
        "PHASE 3 task submission",
        "PHASE 4 waiting for terminal status",
        "PHASE 5 result/log retrieval"
    )) {
    Assert-Contains `
        -Needle $expected `
        -Message "live E2E harness must implement '$expected'."
}

# Server-assigned identity contract.
Assert-Contains `
    -Needle "hm-worker-" `
    -Message "live E2E harness must verify the server-assigned hm-worker-* identity."
Assert-Contains `
    -Needle "hm-enroll-v1." `
    -Message "live E2E harness must verify the versioned enrollment credential prefix."

# Enforce-mode settlement gates. A completed-but-unsettled task is a failure,
# never a warning.
foreach ($expected in @(
        "billing_settled=false; settlement did not occur",
        '$usageUnits -le 0',
        '$terminal.status -ne "COMPLETED"'
    )) {
    Assert-Contains `
        -Needle $expected `
        -Message "live E2E harness must fail closed on missing evidence via '$expected'."
}

# Redaction-by-construction: bearer material is guarded before any write and
# never formatted into output.
Assert-Contains `
    -Needle 'function Assert-Redacted' `
    -Message "live E2E harness must carry the redaction guard."
Assert-Contains `
    -Needle "refusing to write it" `
    -Message "live E2E harness must refuse to persist bearer material."

# The harness must not accept local substitutes for external evidence.
foreach ($forbidden in @(
        "docker compose",
        "localhost:50051",
        "127\.0\.0\.1:5005",
        "MANAGED_PROOF_ROLLOUT_MODE=off",
        "rollout mode observe"
    )) {
    Assert-NotContains `
        -Pattern $forbidden `
        -Message "live E2E harness must not embed a local substitute ('$forbidden')."
}

# Evidence records digests/sizes/states, not payloads or credentials.
Assert-Contains `
    -Needle "result_json_sha256 = `$resultJsonSha256" `
    -Message "live E2E harness must store the computed result JSON digest, not a placeholder."
Assert-Contains `
    -Needle "Get-Sha256Hex -Bytes ([System.Text.Encoding]::UTF8.GetBytes(`$resultJson))" `
    -Message "live E2E harness must hash the exact retrieved result JSON bytes."
Assert-Contains `
    -Needle "`$resultJsonSha256 -notmatch '^[0-9a-f]{64}$'" `
    -Message "live E2E harness must reject missing or malformed result digests."

Write-Host "managed proof live E2E harness contract tests passed"
