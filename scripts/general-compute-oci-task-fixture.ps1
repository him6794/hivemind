[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("provision", "execute")]
    [string]$Phase,
    [Parameter(Mandatory = $true)][string]$ComposeProject,
    [Parameter(Mandatory = $true)][string]$ComposeFile,
    [Parameter(Mandatory = $true)][string]$RegistryPath,
    [Parameter(Mandatory = $true)][string]$EvidencePath,
    [Parameter(Mandatory = $true)][string]$TaskId,
    [Parameter(Mandatory = $true)][string]$MasterBaseUrl,
    [Parameter(Mandatory = $true)][string]$WorkerControlBaseUrl,
    [Parameter(Mandatory = $true)][string]$Username,
    [Parameter(Mandatory = $true)][string]$Password,
    [Parameter(Mandatory = $true)][string]$ConfigVolumeName,
    [Parameter(Mandatory = $true)][string]$StateVolumeName,
    [Parameter(Mandatory = $true)][string]$WorkerId
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot

function Fail-Fixture {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "reviewed general-compute OCI fixture failed closed: $Message"
}

function Require-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (!(Get-Command $Name -ErrorAction SilentlyContinue)) {
        Fail-Fixture "required command '$Name' is not installed"
    }
}

function Require-AbsolutePath {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if (![IO.Path]::IsPathRooted($Path)) {
        Fail-Fixture "$Name must be an absolute path"
    }
}

function Require-RegularFile {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    Require-AbsolutePath $Name $Path
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) {
        Fail-Fixture "$Name is missing or is not a regular file: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail-Fixture "$Name must not be a symlink/reparse point: $Path"
    }
}

function Require-Directory {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    Require-AbsolutePath $Name $Path
    if (!(Test-Path -LiteralPath $Path -PathType Container)) {
        Fail-Fixture "$Name is missing or is not a directory: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail-Fixture "$Name must not be a symlink/reparse point: $Path"
    }
}

function Read-JsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    Require-RegularFile $Name $Path
    try {
        return (Get-Content -LiteralPath $Path -Raw -Encoding utf8 | ConvertFrom-Json)
    } catch {
        Fail-Fixture "$Name is not valid JSON: $($_.Exception.Message)"
    }
}

function Write-Utf8Json {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )
    $json = $Value | ConvertTo-Json -Depth 100 -Compress
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($Path, $json, $encoding)
}

function Copy-TreeWithoutReparsePoints {
    param(
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    Require-Directory "operator rootfs" $Source
    foreach ($entry in @(Get-ChildItem -LiteralPath $Source -Force -Recurse)) {
        if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Fail-Fixture "operator rootfs contains a symlink/reparse point: $($entry.FullName)"
        }
    }
    if (!(Test-Path -LiteralPath $Destination -PathType Container)) {
        New-Item -ItemType Directory -LiteralPath $Destination -Force | Out-Null
    }
    foreach ($entry in @(Get-ChildItem -LiteralPath $Source -Force)) {
        Copy-Item -LiteralPath $entry.FullName `
            -Destination (Join-Path $Destination $entry.Name) -Recurse -Force
    }
}

function Copy-VerifiedFile {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Source,
        [Parameter(Mandatory = $true)][string]$Destination
    )
    Require-RegularFile $Name $Source
    $parent = Split-Path -Parent $Destination
    if (!(Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -LiteralPath $parent -Force | Out-Null
    }
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
}

function Invoke-Compose {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)
    $output = @(& docker compose `
        --project-name $ComposeProject `
        --project-directory $repoRoot `
        --file $ComposeFile `
        @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        $diagnostics = ($output | ForEach-Object { $_.ToString() }) -join "`n"
        Fail-Fixture "docker compose $($Arguments -join ' ') failed with exit code $exitCode`n$diagnostics"
    }
    return $output
}

function Invoke-Docker {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)
    $output = @(& docker @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        $diagnostics = ($output | ForEach-Object { $_.ToString() }) -join "`n"
        Fail-Fixture "docker $($Arguments -join ' ') failed with exit code $exitCode`n$diagnostics"
    }
    return $output
}

function Provision-OperatorVolumes {
    Require-Command "docker"
    $registrations = @(Read-JsonFile "operator production backend registry" $RegistryPath)
    if ($registrations.Count -eq 0) {
        Fail-Fixture "operator production backend registry is empty"
    }

    $stagingRoot = Join-Path ([IO.Path]::GetTempPath()) (
        "hivemind-general-compute-oci-provision-" + [guid]::NewGuid().ToString("N")
    )
    $configRoot = Join-Path $stagingRoot "config"
    try {
        New-Item -ItemType Directory -LiteralPath $configRoot -Force | Out-Null
        $transformed = @()
        foreach ($registration in $registrations) {
            $backendId = [string]$registration.backend_id
            if ([string]::IsNullOrWhiteSpace($backendId) -or
                $backendId -notmatch '^[A-Za-z0-9_.-]+$') {
                Fail-Fixture "operator registry contains an unsafe backend id"
            }
            $bundle = Join-Path $configRoot (Join-Path "bundles" $backendId)
            Copy-TreeWithoutReparsePoints `
                (Join-Path ([string]$registration.bundle_root) "rootfs") `
                (Join-Path $bundle "rootfs")
            Copy-VerifiedFile "backend '$backendId' pinned runner" `
                ([string]$registration.runner_executable) `
                (Join-Path $configRoot (Join-Path (Join-Path "runners" $backendId) "runner"))
            Copy-VerifiedFile "backend '$backendId' seccomp profile" `
                ([string]$registration.seccomp_profile_path) `
                (Join-Path $configRoot (Join-Path "seccomp" "$backendId.json"))

            $properties = [ordered]@{}
            foreach ($property in $registration.PSObject.Properties) {
                $properties[$property.Name] = $property.Value
            }
            $mapped = [pscustomobject]$properties
            $mapped.bundle_root = "/etc/hivemind/general-compute/bundles/$backendId"
            $mapped.artifact_root = "/var/lib/hivemind/general-compute/artifacts/$backendId"
            $mapped.runner_executable = "/etc/hivemind/general-compute/runners/$backendId/runner"
            $mapped.runner_state_root = "/var/lib/hivemind/general-compute/runner-state/$backendId"
            $mapped.seccomp_profile_path = "/etc/hivemind/general-compute/seccomp/$backendId.json"
            $transformed += $mapped
        }
        Write-Utf8Json (Join-Path $configRoot "backends.json") @($transformed)

        # Build the reviewed Worker image first, then use that exact image as
        # the privileged volume-seeding helper. The helper shell only copies
        # operator files; no task command is ever passed to the Worker service.
        [void](Invoke-Compose @("build", "worker"))
        $imageLines = @(Invoke-Compose @("images", "-q", "worker"))
        $image = ($imageLines | ForEach-Object { $_.ToString().Trim() } |
            Where-Object { $_ -match '^[0-9a-fA-F]{12,64}$' } | Select-Object -Last 1)
        if ([string]::IsNullOrWhiteSpace([string]$image)) {
            Fail-Fixture "docker compose did not return a Worker image id for volume provisioning"
        }
        $stateDirectories = @("mkdir -p /state/cas")
        foreach ($registration in $transformed) {
            $backendId = [string]$registration.backend_id
            $stateDirectories += "mkdir -p /state/artifacts/$backendId /state/runner-state/$backendId"
        }
        $seedCommandParts = @(
            "set -eu",
            "rm -rf /target/*",
            "cp -a /source/config/. /target/",
            "mkdir -p /state/artifacts /state/runner-state"
        ) + $stateDirectories + @(
            "chown -R 10001:10001 /state",
            "chmod -R a+rX /target",
            "chmod a+rx /target/runners/*/runner"
        )
        $seedCommand = $seedCommandParts -join "; "
        [void](Invoke-Docker @(
            "run", "--rm", "--user", "0:0",
            "-v", "$ConfigVolumeName`:/target",
            "-v", "$StateVolumeName`:/state",
            "-v", "$stagingRoot`:/source:ro",
            $image,
            "/bin/sh", "-c", $seedCommand
        ))
    } finally {
        if (Test-Path -LiteralPath $stagingRoot) {
            Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

function Invoke-Api {
    param(
        [Parameter(Mandatory = $true)][ValidateSet("GET", "POST")][string]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [string]$Token,
        $Body
    )
    $headers = @{}
    if (![string]::IsNullOrWhiteSpace($Token)) {
        $headers.Authorization = "Bearer $Token"
    }
    try {
        if ($null -eq $Body) {
            return Invoke-RestMethod -Method $Method -Uri $Uri -Headers $headers -UseBasicParsing
        }
        $json = $Body | ConvertTo-Json -Depth 100 -Compress
        return Invoke-RestMethod -Method $Method -Uri $Uri -Headers $headers `
            -ContentType "application/json" -Body $json -UseBasicParsing
    } catch {
        Fail-Fixture "$Method $Uri failed: $($_.Exception.Message)"
    }
}

function Wait-Http {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        try {
            $response = Invoke-RestMethod -Method Get -Uri $Uri -UseBasicParsing
            if ($null -ne $response) {
                return $response
            }
        } catch {
            Start-Sleep -Seconds 2
        }
    } while ((Get-Date) -lt $deadline)
    Fail-Fixture "timed out waiting for HTTP endpoint $Uri"
}

function Login-Master {
    $deadline = (Get-Date).AddSeconds(180)
    do {
        try {
            $response = Invoke-Api -Method POST -Uri "$MasterBaseUrl/api/login" -Body @{
                username = $Username
                password = $Password
            }
            if ($response.success -and ![string]::IsNullOrWhiteSpace([string]$response.token)) {
                return [string]$response.token
            }
        } catch {
            # Master may be listening before its Nodepool gRPC dependency is
            # ready; keep login bounded and retry rather than racing startup.
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    Fail-Fixture "Master login did not return a Nodepool-issued token before the deadline"
}

function Wait-WorkerRegistration {
    param([Parameter(Mandatory = $true)][string]$Token)
    $deadline = (Get-Date).AddSeconds(180)
    do {
        try {
            $response = Invoke-Api -Method GET `
                -Uri "$MasterBaseUrl/api/workers?include_offline=true" -Token $Token
            $worker = @($response.workers | Where-Object {
                [string]$_.worker_id -eq $WorkerId
            }) | Select-Object -First 1
            if ($null -ne $worker) {
                return $worker
            }
        } catch {
            # Master/nodepool may still be starting; keep the bounded retry.
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    Fail-Fixture "Worker '$WorkerId' did not register with Nodepool before the deadline"
}

function Read-ExecutionPlan {
    $planPath = [string]$env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES
    if ([string]::IsNullOrWhiteSpace($planPath)) {
        Fail-Fixture "set HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES to a reviewed manifest/case plan"
    }
    return (Read-JsonFile "reviewed OCI task/case plan" $planPath)
}

function Submit-GeneralComputeTask {
    param(
        [Parameter(Mandatory = $true)][string]$Token,
        [Parameter(Mandatory = $true)][string]$CaseTaskId,
        [Parameter(Mandatory = $true)]$Manifest,
        [int64]$MaxCpt = 1000
    )
    $response = Invoke-Api -Method POST -Uri "$MasterBaseUrl/api/tasks" -Token $Token -Body @{
        task_id = $CaseTaskId
        runtime = "general-compute-v1alpha1"
        general_compute_manifest_json = $Manifest
        host_count = 1
        max_cpt = $MaxCpt
    }
    if (!$response.success) {
        Fail-Fixture "task '$CaseTaskId' was rejected: $($response.message)"
    }
}

function Wait-TaskTerminal {
    param(
        [Parameter(Mandatory = $true)][string]$Token,
        [Parameter(Mandatory = $true)][string]$CaseTaskId,
        [int]$TimeoutSeconds = 240
    )
    $terminal = @("COMPLETED", "FAILED", "CANCELLED", "TIMED_OUT")
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $response = Invoke-Api -Method GET -Uri "$MasterBaseUrl/api/tasks" -Token $Token
        $task = @($response.tasks | Where-Object {
            [string]$_.task_id -eq $CaseTaskId
        }) | Select-Object -First 1
        if ($null -ne $task -and $terminal -contains ([string]$task.status).ToUpperInvariant()) {
            return $task
        }
        Start-Sleep -Seconds 2
    } while ((Get-Date) -lt $deadline)
    Fail-Fixture "task '$CaseTaskId' did not reach a terminal state before the deadline"
}

function Read-ResultEnvelope {
    param(
        [Parameter(Mandatory = $true)][string]$CaseTaskId,
        [Parameter(Mandatory = $true)][string]$ExpectedStatus
    )
    if ($CaseTaskId -notmatch '^[A-Za-z0-9_.-]+$') {
        Fail-Fixture "unsafe task id in Postgres result query"
    }
    $sql = "SELECT encode(result_json, 'base64') FROM general_compute_results WHERE task_id = '$CaseTaskId';"
    $lines = @(Invoke-Compose @("exec", "-T", "postgres", "psql", "-U", "hivemind", "-d", "hivemind", "-At", "-c", $sql))
    $encoded = ($lines | ForEach-Object { $_.ToString().Trim() } |
        Where-Object { $_ -and $_ -notmatch '^NOTICE:' } | Select-Object -Last 1)
    if ([string]::IsNullOrWhiteSpace([string]$encoded)) {
        Fail-Fixture "Nodepool did not persist a typed general-compute result for '$CaseTaskId'"
    }
    try {
        $bytes = [Convert]::FromBase64String([string]$encoded)
        $result = ([Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json)
    } catch {
        Fail-Fixture "persisted result for '$CaseTaskId' is not valid UTF-8 JSON"
    }
    if ([string]$result.status -ne $ExpectedStatus) {
        Fail-Fixture "typed result for '$CaseTaskId' has status '$($result.status)', expected '$ExpectedStatus'"
    }
    foreach ($field in @("execution_id", "attempt_id", "idempotency_key", "request_digest", "runtime_version", "backend_id", "guest_image_digest", "input_sha256", "output_manifest_root")) {
        if ([string]::IsNullOrWhiteSpace([string]$result.$field)) {
            Fail-Fixture "typed result for '$CaseTaskId' is missing bound field '$field'"
        }
    }
    return $result
}

function Assert-Settlement {
    param([Parameter(Mandatory = $true)][string]$CaseTaskId)
    if ($CaseTaskId -notmatch '^[A-Za-z0-9_.-]+$') {
        Fail-Fixture "unsafe task id in Postgres settlement query"
    }
    $sql = "SELECT COUNT(*) FROM general_compute_settlements WHERE task_id = '$CaseTaskId' AND evidence_level = 'unverified';"
    $lines = @(Invoke-Compose @("exec", "-T", "postgres", "psql", "-U", "hivemind", "-d", "hivemind", "-At", "-c", $sql))
    $countText = ($lines | ForEach-Object { $_.ToString().Trim() } |
        Where-Object { $_ -match '^[0-9]+$' } | Select-Object -Last 1)
    if ([string]::IsNullOrWhiteSpace([string]$countText) -or [int64]$countText -ne 1) {
        Fail-Fixture "Nodepool did not persist exactly one unverified settlement for '$CaseTaskId'"
    }
}

function Invoke-ExecutionPhase {
    Require-Command "docker"
    $plan = Read-ExecutionPlan
    if ($null -eq $plan.primary_manifest) {
        Fail-Fixture "reviewed OCI task/case plan must contain primary_manifest"
    }
    Wait-Http "$MasterBaseUrl/health" 180 | Out-Null
    Wait-Http "$WorkerControlBaseUrl/api/worker-info" 180 | Out-Null
    $token = Login-Master
    [void](Wait-WorkerRegistration -Token $token)

    $maxCpt = 1000
    if ($null -ne $plan.max_cpt) {
        $maxCpt = [int64]$plan.max_cpt
    }
    Submit-GeneralComputeTask -Token $token -CaseTaskId $TaskId `
        -Manifest $plan.primary_manifest -MaxCpt $maxCpt
    $primaryTask = Wait-TaskTerminal -Token $token -CaseTaskId $TaskId
    if ([string]$primaryTask.status -ne "COMPLETED") {
        Fail-Fixture "primary OCI task '$TaskId' did not complete: $($primaryTask.status)"
    }
    $primaryResult = Read-ResultEnvelope -CaseTaskId $TaskId -ExpectedStatus "completed"
    Assert-Settlement -CaseTaskId $TaskId

    $caseResults = [ordered]@{}
    foreach ($caseName in @("timeout_cancel", "network_denied", "filesystem_denied")) {
        $case = $plan.PSObject.Properties[$caseName]
        if ($null -eq $case -or $null -eq $case.Value -or $null -eq $case.Value.manifest) {
            Fail-Fixture "reviewed OCI task/case plan must contain '$caseName.manifest'"
        }
        $caseTaskId = "$TaskId-$caseName"
        $expectedTaskStatus = [string]$case.Value.expected_task_status
        $expectedResultStatus = [string]$case.Value.expected_result_status
        if ([string]::IsNullOrWhiteSpace($expectedTaskStatus) -or
            [string]::IsNullOrWhiteSpace($expectedResultStatus)) {
            Fail-Fixture "case '$caseName' must declare expected_task_status and expected_result_status"
        }
        Submit-GeneralComputeTask -Token $token -CaseTaskId $caseTaskId `
            -Manifest $case.Value.manifest -MaxCpt $maxCpt
        if ($caseName -eq "timeout_cancel") {
            $delay = 1
            if ($null -ne $case.Value.cancel_after_seconds) {
                $delay = [int]$case.Value.cancel_after_seconds
            }
            Start-Sleep -Seconds ([Math]::Max(0, $delay))
            $stop = Invoke-Api -Method POST -Uri "$MasterBaseUrl/api/tasks/$caseTaskId/stop" -Token $token
            if (!$stop.success) {
                Fail-Fixture "timeout/cancel case '$caseTaskId' did not accept stop request"
            }
        }
        $terminal = Wait-TaskTerminal -Token $token -CaseTaskId $caseTaskId
        if ([string]$terminal.status -ne $expectedTaskStatus) {
            Fail-Fixture "case '$caseName' reached '$($terminal.status)', expected '$expectedTaskStatus'"
        }
        $caseResults[$caseName] = Read-ResultEnvelope `
            -CaseTaskId $caseTaskId -ExpectedStatus $expectedResultStatus
    }

    $evidence = [ordered]@{
        schema_version = "general-compute-oci-e2e-v1"
        project_name = $ComposeProject
        task_id = $TaskId
        status = "passed"
        services = [ordered]@{
            postgres = "postgres"
            nodepool = "nodepool"
            master = "master"
            worker = "worker"
        }
        checks = [ordered]@{
            worker_registered = $true
            task_completion = $true
            postgres_settlement = $true
            timeout_cancel = $true
            network_denied = $true
            filesystem_denied = $true
        }
        result = [ordered]@{
            kind = "general-compute-result-v1"
            # The Worker only persists a validated GeneralComputeResult after
            # decoding the runner's ProductionResultEnvelope. These bound
            # fields are the durable evidence of that decoder path.
            production_result_envelope = $true
            execution_id = [string]$primaryResult.execution_id
            attempt_id = [string]$primaryResult.attempt_id
            request_digest = [string]$primaryResult.request_digest
            status = [string]$primaryResult.status
            input_sha256 = [string]$primaryResult.input_sha256
            output_manifest_root = [string]$primaryResult.output_manifest_root
        }
        observations = [ordered]@{
            worker_control = "$WorkerControlBaseUrl/api/worker-info"
            master = $MasterBaseUrl
            hostile_case_task_ids = @($caseResults.Keys | ForEach-Object { "$TaskId-$_" })
        }
    }
    Write-Utf8Json $EvidencePath $evidence
}

if ($Phase -eq "provision") {
    Provision-OperatorVolumes
} else {
    Invoke-ExecutionPhase
}
