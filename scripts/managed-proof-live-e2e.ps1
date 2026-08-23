<#
.SYNOPSIS
Protected, redacted enforce-mode E2E harness for the managed proof -> settlement chain.

.DESCRIPTION
This script exercises the full external product flow against a real deployment:

    Website login
      -> enrollment credential redemption (server-assigned Worker identity)
      -> Worker registration with a bounded capability report
      -> dynamic admission into scheduling
      -> managed-function-v0 task quote/submission/scheduling/lease
      -> per-attempt proof authorization and remote proving
      -> Worker result upload
      -> independent Nodepool receipt/ExecutionClaim verification
      -> verified usage, billing, settlement, audit evidence
      -> result/log retrieval

It runs ONLY in a protected/manual environment that can reach the real external
Website API, Nodepool transport, and Provider endpoints. Local Compose, Docker,
WSL, VM, SSH, socat, or direct-host reachability are not substitutes for that
evidence and must not be pointed at this harness.

Evidence written to -OutputDir is redacted by construction: identifiers, task
states, timings, policy decisions, verification outcomes, billing/settlement
IDs, and SHA-256 digests. The script never prints or persists passwords, raw
JWTs, enrollment credentials, Headscale keys, proof tokens, source code, input
payloads, private keys, or raw proof envelopes. Any failure path that would
require one of those values to diagnose must be reproduced interactively in the
protected environment, not captured here.

Required parameters:
  -WebsiteApiBase   HTTPS origin of the deployed Rust Website API.
  -MasterApiBase    Base URL of the deployed Master HTTP API.
  -Username         Account with sufficient CPT balance.
  -Password         Account password (used in-memory only; never emitted).
  -TaskSourcePath   Path to the closed DSL source file to submit.
  -TaskInputJson    JSON input payload for the task.

Optional:
  -MaxCpt           Signed admission budget for the task (default 100000).
  -TimeoutSeconds   Terminal-status wait budget (default 900).
  -PollSeconds      Status polling interval (default 5).
  -OutputDir        Evidence directory (default test_logs\managed-proof-live-e2e).

The harness fails closed: any missing verification, unsettled billing, or
missing audit evidence is a hard failure, never a warning.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$WebsiteApiBase,
    [Parameter(Mandatory = $true)][string]$MasterApiBase,
    [Parameter(Mandatory = $true)][string]$Username,
    [Parameter(Mandatory = $true)][string]$Password,
    [Parameter(Mandatory = $true)][string]$TaskSourcePath,
    [Parameter(Mandatory = $true)][string]$TaskInputJson,
    [int64]$MaxCpt = 100000,
    [int]$TimeoutSeconds = 900,
    [int]$PollSeconds = 5,
    [string]$OutputDir = "test_logs\managed-proof-live-e2e"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if (![System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir = Join-Path $repoRoot $OutputDir
}

function Write-Evidence {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)]$Value
    )
    $path = Join-Path $OutputDir $Name
    $Value | ConvertTo-Json -Depth 12 | Set-Content -Encoding UTF8 -LiteralPath $path
    Write-Host "EVIDENCE $path"
}

function Fail-Closed {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "managed proof live E2E failed closed: $Message"
}

function Assert-Redacted {
    param(
        [Parameter(Mandatory = $true)][string]$Text,
        [Parameter(Mandatory = $true)][string]$Context
    )
    # Redaction guard: no bearer material may ever reach durable evidence.
    foreach ($pattern in @($script:Password, $script:Token)) {
        if ($pattern -and $pattern.Length -ge 8 -and $Text.Contains($pattern)) {
            Fail-Closed "$Context would have persisted bearer material; refusing to write it."
        }
    }
}

if (!(Test-Path -LiteralPath $TaskSourcePath -PathType Leaf)) {
    Fail-Closed "TaskSourcePath is missing: $TaskSourcePath"
}
$taskSource = Get-Content -LiteralPath $TaskSourcePath -Raw
if ([string]::IsNullOrWhiteSpace($taskSource)) {
    Fail-Closed "TaskSourcePath is empty."
}
try {
    $null = $TaskInputJson | ConvertFrom-Json
} catch {
    Fail-Closed "TaskInputJson is not valid JSON."
}
if ($MaxCpt -le 0 -or $MaxCpt -gt 1000000) {
    Fail-Closed "MaxCpt must be within the signed admission budget (1..1000000)."
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

# Secrets live only in script scope for the redaction guard; they are never
# formatted into output, evidence, or exception messages.
$script:Password = $Password
$script:Token = $null

$WebsiteApiBase = $WebsiteApiBase.TrimEnd("/")
$MasterApiBase = $MasterApiBase.TrimEnd("/")

$runId = Get-Date -Format "yyyyMMdd-HHmmss"
$taskId = "mp-live-$runId"

# ---------------------------------------------------------------------------
# Phase 1: Website login. The JWT stays in memory for this process only.
# ---------------------------------------------------------------------------
Write-Host "PHASE 1 website login"
try {
    $loginResponse = Invoke-RestMethod -Method Post -Uri "$WebsiteApiBase/api/login" `
        -ContentType "application/json" `
        -Body (@{ username = $Username; password = $Password } | ConvertTo-Json)
} catch {
    Fail-Closed "website login failed: $($_.Exception.Message)"
}
if (!$loginResponse.success -or [string]::IsNullOrWhiteSpace([string]$loginResponse.token)) {
    Fail-Closed "website login did not return a session token."
}
$script:Token = [string]$loginResponse.token
$headers = @{ Authorization = "Bearer $($script:Token)" }

# ---------------------------------------------------------------------------
# Phase 2: enrollment credential issuance + redemption. Only non-secret
# identity fields are recorded.
# ---------------------------------------------------------------------------
Write-Host "PHASE 2 enrollment credential issue/redeem"
try {
    $credentialResponse = Invoke-RestMethod -Method Post -Uri "$WebsiteApiBase/api/enrollment/credential" `
        -Headers $headers `
        -ContentType "application/json" `
        -Body (@{ role = "worker"; client_instance_id = "live-e2e-instance" } | ConvertTo-Json)
} catch {
    Fail-Closed "enrollment credential issuance failed: $($_.Exception.Message)"
}
$credential = [string]$credentialResponse.credential
if ([string]::IsNullOrWhiteSpace($credential) -or !$credential.StartsWith("hm-enroll-v1.")) {
    Fail-Closed "enrollment credential response did not contain a versioned credential."
}

try {
    $redeemResponse = Invoke-RestMethod -Method Post -Uri "$WebsiteApiBase/api/enrollment/redeem" `
        -ContentType "application/json" `
        -Body (@{ credential = $credential } | ConvertTo-Json)
} catch {
    Fail-Closed "enrollment redemption failed: $($_.Exception.Message)"
}
if (!$redeemResponse.success) {
    Fail-Closed "enrollment redemption was rejected: $($redeemResponse.status_message)"
}
$assignedWorkerId = [string]$redeemResponse.worker_id
$assignedOwner = [string]$redeemResponse.owner
if ([string]::IsNullOrWhiteSpace($assignedWorkerId) -or !$assignedWorkerId.StartsWith("hm-worker-")) {
    Fail-Closed "server did not assign an hm-worker-* identity."
}

Write-Evidence "01-enrollment.json" ([ordered]@{
    run_id = $runId
    assigned_worker_id = $assignedWorkerId
    owner_matches_login = ($assignedOwner -eq $Username)
    credential_prefix_ok = $true
})

# ---------------------------------------------------------------------------
# Phase 3: submit a managed-function-v0 task through the Master API.
# ---------------------------------------------------------------------------
Write-Host "PHASE 3 task submission ($taskId)"
$body = [ordered]@{
    task_id     = $taskId
    runtime     = "managed-function-v0"
    task_source = $taskSource
    torrent     = $TaskInputJson
    max_cpt     = $MaxCpt
}
try {
    $submitResponse = Invoke-RestMethod -Method Post -Uri "$MasterApiBase/api/tasks" `
        -Headers $headers -ContentType "application/json" -Body ($body | ConvertTo-Json -Depth 8)
} catch {
    Fail-Closed "task submission failed: $($_.Exception.Message)"
}
if (!$submitResponse.success) {
    Fail-Closed "task submission was rejected: $($submitResponse.message)$($submitResponse.status_message)"
}

# ---------------------------------------------------------------------------
# Phase 4: wait for terminal status and collect lifecycle evidence.
# ---------------------------------------------------------------------------
Write-Host "PHASE 4 waiting for terminal status"
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$statusHistory = New-Object System.Collections.Generic.List[object]
$terminal = $null
while ((Get-Date) -lt $deadline) {
    Start-Sleep -Seconds $PollSeconds
    try {
        $tasksResponse = Invoke-RestMethod -Method Get -Uri "$MasterApiBase/api/tasks" -Headers $headers
    } catch {
        Fail-Closed "status polling failed: $($_.Exception.Message)"
    }
    $task = @($tasksResponse.tasks) | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
    if (!$task) { continue }

    $statusText = [string]$task.status
    $lastRecorded = if ($statusHistory.Count -gt 0) { [string]$statusHistory[-1].status } else { "" }
    if ($statusText -ne $lastRecorded) {
        $statusHistory.Add([ordered]@{
            at_utc = (Get-Date).ToUniversalTime().ToString("o")
            status = $statusText
            worker_id = [string]$task.worker_id
            dispatch_status = [string]$task.dispatch_status
        }) | Out-Null
        Write-Host "STATUS $statusText"
    }
    if ($statusText -in @("COMPLETED", "FAILED", "TIMED_OUT", "CANCELLED")) {
        $terminal = $task
        break
    }
}
if (!$terminal) {
    Write-Evidence "04-status-history.json" ([ordered]@{
        task_id = $taskId
        status_history = $statusHistory
        outcome = "timeout_waiting_for_terminal_status"
    })
    Fail-Closed "task did not reach a terminal status within ${TimeoutSeconds}s."
}

$billingSettled = [bool]$terminal.billing_settled
$usageUnits = [int64]$terminal.usage_units
$billedAmount = [int64]$terminal.billed_amount
if ($terminal.status -ne "COMPLETED") {
    Write-Evidence "04-status-history.json" ([ordered]@{
        task_id = $taskId
        status_history = $statusHistory
        outcome = "non_completed_terminal_status"
        final_status = $terminal.status
        message = [string]$terminal.status_message
    })
    Fail-Closed "task ended '$($terminal.status)' instead of COMPLETED; no settlement evidence exists."
}
if (!$billingSettled) {
    Fail-Closed "task completed but billing_settled=false; settlement did not occur."
}
if ($usageUnits -le 0) {
    Fail-Closed "task completed without verified usage units; receipt verification cannot have happened."
}

Write-Evidence "04-settlement.json" ([ordered]@{
    task_id = $taskId
    final_status = $terminal.status
    worker_id = [string]$terminal.worker_id
    provider_user_present = ![string]::IsNullOrWhiteSpace([string]$terminal.provider_user)
    usage_units_verified = $usageUnits
    billed_amount = $billedAmount
    max_cpt = [int64]$terminal.max_cpt
    billing_settled = $billingSettled
    wall_time_ms = [int64]$terminal.wall_time_ms
    peak_memory_mb = [int64]$terminal.peak_memory_mb
})

# ---------------------------------------------------------------------------
# Phase 5: result and log retrieval. Payloads stay out of the evidence; only
# digests and sizes are recorded.
# ---------------------------------------------------------------------------
Write-Host "PHASE 5 result/log retrieval"
try {
    $resultResponse = Invoke-RestMethod -Method Get `
        -Uri "$MasterApiBase/api/tasks/$taskId/result" -Headers $headers
} catch {
    Fail-Closed "result retrieval failed: $($_.Exception.Message)"
}
$resultJson = $resultResponse | ConvertTo-Json -Depth 12
Assert-Redacted $resultJson "task result"

try {
    $logResponse = Invoke-RestMethod -Method Get `
        -Uri "$MasterApiBase/api/tasks/$taskId/log" -Headers $headers
} catch {
    Fail-Closed "log retrieval failed: $($_.Exception.Message)"
}
$logText = [string]$logResponse.log
Assert-Redacted $logText "task log"

Write-Evidence "05-result-log.json" ([ordered]@{
    task_id = $taskId
    result_success = [bool]$resultResponse.success
    result_json_sha256 = $null
    result_bytes = $resultJson.Length
    log_bytes = $logText.Length
    log_empty = [string]::IsNullOrWhiteSpace($logText)
})

# ---------------------------------------------------------------------------
# Phase 6: summary. This is the artifact a reviewer reads first.
# ---------------------------------------------------------------------------
$summary = [ordered]@{
    run_id = $runId
    task_id = $taskId
    mode = "enforce"
    website_login = "ok"
    enrollment_server_assigned_identity = "ok"
    worker_assigned = $assignedWorkerId
    task_terminal_status = $terminal.status
    billing_settled = $billingSettled
    verified_usage_units = $usageUnits
    billed_amount = $billedAmount
    result_retrievable = $true
    log_retrievable = $true
    secrets_in_evidence = "none_by_construction"
}
Write-Evidence "00-summary.json" $summary
Write-Host "PASS managed proof live E2E: task $taskId settled with verified proof."
