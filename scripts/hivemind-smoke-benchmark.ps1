param(
    [Parameter(Mandatory = $true)][string]$MasterUrl,
    [Parameter(Mandatory = $true)][string]$Token,
    [Parameter(Mandatory = $true)][string]$TaskSourcePath,
    [string]$TaskInputPath,
    [string]$Runtime = "managed-function-v0",
    [int[]]$WorkerCounts = @(1, 5),
    [int[]]$TaskCounts = @(10, 100),
    [int]$CpuScore = 1,
    [int]$MemoryGb = 1,
    [int]$StorageGb = 1,
    [int]$MaxCpt = 1000,
    [int]$PollSeconds = 2,
    [int]$TimeoutSeconds = 600,
    [string]$OutputDir = "test_logs\smoke-benchmark"
)

$ErrorActionPreference = "Stop"

if (!(Test-Path -LiteralPath $TaskSourcePath)) {
    throw "TaskSourcePath not found: $TaskSourcePath"
}
$taskSource = Get-Content -LiteralPath $TaskSourcePath -Raw
if ([string]::IsNullOrWhiteSpace($taskSource)) {
    throw "TaskSourcePath is empty: $TaskSourcePath"
}

$taskInput = $null
if ($TaskInputPath) {
    if (!(Test-Path -LiteralPath $TaskInputPath)) {
        throw "TaskInputPath not found: $TaskInputPath"
    }
    $taskInput = Get-Content -LiteralPath $TaskInputPath -Raw
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$MasterUrl = $MasterUrl.TrimEnd("/")
$headers = @{ Authorization = "Bearer $Token" }

function Invoke-HivemindGet {
    param([Parameter(Mandatory = $true)][string]$Path)
    Invoke-RestMethod -Method Get -Uri "$MasterUrl$Path" -Headers $headers
}

function Submit-HivemindTask {
    param([Parameter(Mandatory = $true)][string]$TaskId)

    # managed-function-v0 tasks carry their JSON input in the `torrent` field;
    # nodepool stores it verbatim as the task input reference.
    $body = [ordered]@{
        task_id     = $TaskId
        runtime     = $Runtime
        task_source = $taskSource
        cpu_score   = $CpuScore
        memory_gb   = $MemoryGb
        storage_gb  = $StorageGb
        max_cpt     = $MaxCpt
    }
    if ($null -ne $taskInput) {
        $body["torrent"] = $taskInput
    }
    $payload = $body | ConvertTo-Json -Depth 12

    $start = [Diagnostics.Stopwatch]::StartNew()
    $response = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks" `
        -Headers $headers `
        -ContentType "application/json" `
        -Body $payload
    $start.Stop()

    [pscustomobject]@{
        response = $response
        submit_latency_ms = $start.ElapsedMilliseconds
    }
}

function Get-TaskById {
    param(
        [Parameter(Mandatory = $true)][object[]]$Tasks,
        [Parameter(Mandatory = $true)][string]$TaskId
    )
    $Tasks | Where-Object { $_.task_id -eq $TaskId -or $_.TaskID -eq $TaskId } | Select-Object -First 1
}

function Get-TaskStatus {
    param([Parameter(Mandatory = $true)][object]$Task)
    $status = $Task.status
    if (!$status) { $status = $Task.Status }
    [string]$status
}

function Is-TerminalStatus {
    param([Parameter(Mandatory = $true)][string]$Status)
    $Status -in @("COMPLETED", "FAILED", "TIMED_OUT", "CANCELLED")
}

$runId = Get-Date -Format "yyyyMMdd-HHmmss"
$rows = New-Object System.Collections.Generic.List[object]

for ($scenarioIndex = 0; $scenarioIndex -lt $WorkerCounts.Count; $scenarioIndex++) {
    $workerTarget = $WorkerCounts[$scenarioIndex]
    $taskTarget = if ($scenarioIndex -lt $TaskCounts.Count) { $TaskCounts[$scenarioIndex] } else { $TaskCounts[-1] }
    $scenario = "w${workerTarget}-t${taskTarget}"
    $taskIds = 1..$taskTarget | ForEach-Object { "smoke-$runId-$scenario-$_" }

    $workers = @()
    try {
        $workerResponse = Invoke-HivemindGet -Path "/api/workers"
        if ($workerResponse.workers) { $workers = @($workerResponse.workers) }
    } catch {
        Write-Warning "Could not read worker list before ${scenario}: $($_.Exception.Message)"
    }

    foreach ($taskId in $taskIds) {
        $submitted = Submit-HivemindTask -TaskId $taskId
        $rows.Add([pscustomobject]@{
            run_id = $runId
            scenario = $scenario
            worker_target = $workerTarget
            observed_workers = $workers.Count
            task_count = $taskTarget
            task_id = $taskId
            submit_latency_ms = $submitted.submit_latency_ms
            terminal_latency_ms = $null
            status = if ($submitted.response.success) { "SUBMITTED" } else { "SUBMIT_FAILED" }
            redispatched = 0
            wall_time_ms = 0
            peak_memory_mb = 0
            message = $submitted.response.message
        }) | Out-Null
    }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $pending = [System.Collections.Generic.HashSet[string]]::new([string[]]$taskIds)
    $terminalStart = Get-Date
    while ($pending.Count -gt 0 -and (Get-Date) -lt $deadline) {
        Start-Sleep -Seconds $PollSeconds
        $taskResponse = Invoke-HivemindGet -Path "/api/tasks"
        $tasks = @($taskResponse.tasks)

        foreach ($taskId in @($pending)) {
            $task = Get-TaskById -Tasks $tasks -TaskId $taskId
            if (!$task) { continue }
            $status = Get-TaskStatus -Task $task
            if (Is-TerminalStatus -Status $status) {
                $row = $rows | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
                $row.terminal_latency_ms = [int64]((Get-Date) - $terminalStart).TotalMilliseconds
                $row.status = $status
                $retryCount = $task.retry_count
                if ($null -eq $retryCount) { $retryCount = 0 }
                $wallTimeMs = $task.wall_time_ms
                if ($null -eq $wallTimeMs) { $wallTimeMs = 0 }
                $peakMemoryMb = $task.peak_memory_mb
                if ($null -eq $peakMemoryMb) { $peakMemoryMb = 0 }
                $statusMessage = $task.status_message
                if ($null -eq $statusMessage) { $statusMessage = "" }
                $row.redispatched = [int]$retryCount
                $row.wall_time_ms = [int64]$wallTimeMs
                $row.peak_memory_mb = [int64]$peakMemoryMb
                $row.message = [string]$statusMessage
                [void]$pending.Remove($taskId)
            }
        }
    }

    foreach ($taskId in @($pending)) {
        $row = $rows | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
        $row.status = "TIMEOUT_WAITING_FOR_TERMINAL_STATUS"
        $row.terminal_latency_ms = $TimeoutSeconds * 1000
    }
}

$csvPath = Join-Path $OutputDir "hivemind-smoke-benchmark-$runId.csv"
$jsonPath = Join-Path $OutputDir "hivemind-smoke-benchmark-$runId.json"
$rows | Export-Csv -NoTypeInformation -Encoding UTF8 -LiteralPath $csvPath
$rows | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -LiteralPath $jsonPath

[pscustomobject]@{
    run_id = $runId
    rows = $rows.Count
    csv = $csvPath
    json = $jsonPath
    completed = @($rows | Where-Object { $_.status -eq "COMPLETED" }).Count
    failed = @($rows | Where-Object { $_.status -eq "FAILED" }).Count
    timed_out = @($rows | Where-Object { $_.status -like "*TIMEOUT*" }).Count
    redispatched = ($rows | Measure-Object -Property redispatched -Sum).Sum
} | ConvertTo-Json -Depth 4
