param(
    [Parameter(Mandatory = $true)][string]$MasterUrl,
    [Parameter(Mandatory = $true)][string]$Token,
    [Parameter(Mandatory = $true)][string]$TaskSourcePath,
    [string]$TaskInputPath,
    [ValidateSet("hcode")][string]$Runtime = "hcode",
    [int[]]$WorkerCounts = @(3, 5),
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

function ConvertTo-BenchmarkOptionalInt64 {
    param([AllowNull()][object]$Value)

    if ($null -eq $Value -or [string]::IsNullOrWhiteSpace([string]$Value)) {
        return $null
    }

    try {
        return [int64]$Value
    } catch {
        return $null
    }
}

function Get-TaskStatus {
    param([Parameter(Mandatory = $true)][object]$Task)

    $status = $Task.status
    if ($null -eq $status) { $status = $Task.Status }
    ([string]$status).Trim().ToUpperInvariant()
}

function Is-TerminalStatus {
    param([AllowEmptyString()][string]$Status)

    $normalizedStatus = $Status.Trim().ToUpperInvariant()
    $normalizedStatus -in @("COMPLETED", "FAILED", "TIMED_OUT", "CANCELLED")
}

function Start-BenchmarkTaskSubmission {
    param(
        [Parameter(Mandatory = $true)][string]$TaskId,
        [Parameter(Mandatory = $true)][scriptblock]$SubmitAction
    )

    # This monotonic clock starts before invoking the submission action so the
    # task's terminal latency includes submission and execution/wait time.
    $taskClock = [Diagnostics.Stopwatch]::StartNew()
    $submitClock = [Diagnostics.Stopwatch]::StartNew()
    $response = $null
    $message = $null
    $status = "SUBMIT_FAILED"

    try {
        $response = & $SubmitAction $TaskId $taskClock
        if ($null -ne $response) {
            $message = ConvertTo-BenchmarkOptionalString -Value $response.message
            if ([bool]$response.success) {
                $status = "SUBMITTED"
            }
        }
    } catch {
        $message = $_.Exception.Message
    } finally {
        $submitClock.Stop()
    }

    if ($status -eq "SUBMIT_FAILED") {
        $taskClock.Stop()
    }

    [pscustomobject]@{
        task_id = $TaskId
        clock = $taskClock
        submit_latency_ms = [int64]$submitClock.ElapsedMilliseconds
        status = $status
        message = $message
    }
}

function ConvertTo-BenchmarkOptionalString {
    param([AllowNull()][object]$Value)

    if ($null -eq $Value) { return $null }
    [string]$Value
}

function Get-BenchmarkPendingTaskIds {
    param([AllowNull()][object[]]$Rows)

    $taskIds = New-Object System.Collections.Generic.List[string]
    foreach ($row in @($Rows)) {
        if ([string]$row.status -eq "SUBMITTED") {
            $taskIds.Add([string]$row.task_id)
        }
    }
    $taskIds.ToArray()
}

function Get-TaskById {
    param(
        [AllowNull()][object[]]$Tasks,
        [Parameter(Mandatory = $true)][string]$TaskId
    )

    foreach ($task in @($Tasks)) {
        if ($task.task_id -eq $TaskId -or $task.TaskID -eq $TaskId) {
            return $task
        }
    }
    return $null
}

function Set-BenchmarkTaskTerminalOutcome {
    param(
        [Parameter(Mandatory = $true)][object]$Row,
        [Parameter(Mandatory = $true)][object]$Task,
        [Parameter(Mandatory = $true)][long]$ElapsedMs
    )

    $status = Get-TaskStatus -Task $Task
    if (!(Is-TerminalStatus -Status $status)) { return $false }

    $Row.status = $status
    $Row.terminal_latency_ms = $ElapsedMs
    $Row.wait_duration_ms = $null
    $Row.redispatched = ConvertTo-BenchmarkOptionalInt64 -Value $Task.retry_count
    $Row.wall_time_ms = ConvertTo-BenchmarkOptionalInt64 -Value $Task.wall_time_ms
    $Row.peak_memory_mb = ConvertTo-BenchmarkOptionalInt64 -Value $Task.peak_memory_mb
    $Row.message = ConvertTo-BenchmarkOptionalString -Value $Task.status_message
    return $true
}

function Set-BenchmarkTaskCensoredOutcome {
    param(
        [Parameter(Mandatory = $true)][object]$Row,
        [Parameter(Mandatory = $true)][long]$WaitDurationMs
    )

    $Row.status = "TIMEOUT_WAITING_FOR_TERMINAL_STATUS"
    $Row.terminal_latency_ms = $null
    $Row.wait_duration_ms = $WaitDurationMs
}

function Get-BenchmarkPercentile {
    param(
        [AllowNull()][object[]]$Rows,
        [Parameter(Mandatory = $true)][string]$Metric,
        [Parameter(Mandatory = $true)][ValidateRange(0, 100)][double]$Percentile,
        [Parameter(Mandatory = $true)][string[]]$EligibleStatuses
    )

    $allowedStatuses = @($EligibleStatuses | ForEach-Object { $_.ToUpperInvariant() })
    $values = New-Object 'System.Collections.Generic.List[double]'
    foreach ($row in @($Rows)) {
        $status = ([string]$row.status).ToUpperInvariant()
        if ($allowedStatuses -notcontains $status) { continue }

        $rawValue = $row.$Metric
        if ($null -eq $rawValue -or [string]::IsNullOrWhiteSpace([string]$rawValue)) { continue }

        try {
            $numericValue = [double]$rawValue
        } catch {
            continue
        }
        if ([double]::IsNaN($numericValue) -or [double]::IsInfinity($numericValue)) { continue }
        $values.Add($numericValue)
    }

    if ($values.Count -eq 0) { return $null }

    $sortedValues = [double[]]$values.ToArray()
    [Array]::Sort($sortedValues)
    # Nearest-rank percentile: rank = ceil(percentile * N), with a minimum rank of 1.
    $rank = [Math]::Max(1, [int][Math]::Ceiling(($Percentile / 100.0) * $sortedValues.Length))
    $sortedValues[$rank - 1]
}

function Get-BenchmarkMetricSum {
    param(
        [AllowNull()][object[]]$Rows,
        [Parameter(Mandatory = $true)][string]$Metric
    )

    [int64]$sum = 0
    $knownCount = 0
    foreach ($row in @($Rows)) {
        $value = ConvertTo-BenchmarkOptionalInt64 -Value $row.$Metric
        if ($null -eq $value) { continue }
        $sum += $value
        $knownCount++
    }

    if ($knownCount -eq 0) { return $null }
    $sum
}

function Get-BenchmarkSummary {
    param([AllowNull()][object[]]$Rows)

    $allRows = @($Rows)
    $successfulSubmissionStatuses = @(
        "SUBMITTED",
        "COMPLETED",
        "FAILED",
        "TIMED_OUT",
        "CANCELLED",
        "TIMEOUT_WAITING_FOR_TERMINAL_STATUS"
    )

    [pscustomobject]@{
        completed = @($allRows | Where-Object { $_.status -eq "COMPLETED" }).Count
        failed = @($allRows | Where-Object { $_.status -eq "FAILED" }).Count
        timed_out = @($allRows | Where-Object { $_.status -in @("TIMED_OUT", "TIMEOUT_WAITING_FOR_TERMINAL_STATUS") }).Count
        cancelled = @($allRows | Where-Object { $_.status -eq "CANCELLED" }).Count
        submit_failed = @($allRows | Where-Object { $_.status -eq "SUBMIT_FAILED" }).Count
        redispatched = Get-BenchmarkMetricSum -Rows $allRows -Metric "redispatched"
        p95_submit_latency_ms = Get-BenchmarkPercentile -Rows $allRows -Metric "submit_latency_ms" -Percentile 95 -EligibleStatuses $successfulSubmissionStatuses
        p95_terminal_latency_ms = Get-BenchmarkPercentile -Rows $allRows -Metric "terminal_latency_ms" -Percentile 95 -EligibleStatuses @("COMPLETED")
    }
}

function Invoke-HivemindSmokeBenchmark {
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
    $script:MasterUrl = $MasterUrl.TrimEnd("/")
    $headers = @{ Authorization = "Bearer $Token" }

    function Invoke-HivemindGet {
        param([Parameter(Mandatory = $true)][string]$Path)
        Invoke-RestMethod -Method Get -Uri "$script:MasterUrl$Path" -Headers $headers
    }

    function Submit-HivemindTask {
        param([Parameter(Mandatory = $true)][string]$TaskId)

        # hcode tasks carry their JSON input in the `torrent` field;
        # Nodepool stores it verbatim as the task input reference.
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
        # Keep the depth minimal: the body is a flat object of scalar fields.
        # Windows PowerShell 5.1's ConvertTo-Json grows roughly linearly with
        # -Depth for strings that contain newlines (task_source read via
        # Get-Content -Raw always does), and hangs at higher depths, so an
        # oversized depth here can stall the submission.
        $payload = $body | ConvertTo-Json -Depth 2

        Invoke-RestMethod -Method Post -Uri "$script:MasterUrl/api/tasks" `
            -Headers $headers `
            -ContentType "application/json" `
            -Body $payload
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
            throw "Could not read the worker list before ${scenario}: $($_.Exception.Message)"
        }
        if ($workers.Count -lt $workerTarget) {
            throw "Scenario ${scenario} requires at least $workerTarget registered Workers for hcode consensus; observed $($workers.Count)."
        }

        $scenarioRows = New-Object System.Collections.Generic.List[object]
        $taskClocks = @{}
        foreach ($taskId in $taskIds) {
            $submission = Start-BenchmarkTaskSubmission -TaskId $taskId -SubmitAction {
                param($submittedTaskId, $taskClock)
                Submit-HivemindTask -TaskId $submittedTaskId
            }
            $row = [pscustomobject]@{
                run_id = $runId
                scenario = $scenario
                worker_target = $workerTarget
                observed_workers = $workers.Count
                task_count = $taskTarget
                task_id = $taskId
                submit_latency_ms = $submission.submit_latency_ms
                terminal_latency_ms = $null
                wait_duration_ms = $null
                status = $submission.status
                redispatched = $null
                wall_time_ms = $null
                peak_memory_mb = $null
                message = $submission.message
            }
            $rows.Add($row)
            $scenarioRows.Add($row)
            $taskClocks[$taskId] = $submission.clock
        }

        $pendingIds = [string[]]@(Get-BenchmarkPendingTaskIds -Rows $scenarioRows.ToArray())
        $pending = [System.Collections.Generic.HashSet[string]]::new($pendingIds)
        while ($pending.Count -gt 0) {
            if ($PollSeconds -gt 0) { Start-Sleep -Seconds $PollSeconds }
            $taskResponse = Invoke-HivemindGet -Path "/api/tasks"
            $tasks = @($taskResponse.tasks)

            foreach ($taskId in @($pending)) {
                $task = Get-TaskById -Tasks $tasks -TaskId $taskId
                if ($null -ne $task) {
                    $taskStatus = Get-TaskStatus -Task $task
                    if (Is-TerminalStatus -Status $taskStatus) {
                        $row = $scenarioRows | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
                        [void](Set-BenchmarkTaskTerminalOutcome -Row $row -Task $task -ElapsedMs $taskClocks[$taskId].ElapsedMilliseconds)
                        $taskClocks[$taskId].Stop()
                        [void]$pending.Remove($taskId)
                        continue
                    }
                }

                $clock = $taskClocks[$taskId]
                if ($clock.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
                    $row = $scenarioRows | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
                    Set-BenchmarkTaskCensoredOutcome -Row $row -WaitDurationMs $clock.ElapsedMilliseconds
                    $clock.Stop()
                    [void]$pending.Remove($taskId)
                }
            }
        }
    }

    $csvPath = Join-Path $OutputDir "hivemind-smoke-benchmark-$runId.csv"
    $jsonPath = Join-Path $OutputDir "hivemind-smoke-benchmark-$runId.json"
    # Each row and the summary are flat (scalar fields only); -Depth 2 covers
    # the array-of-rows one level deep. Higher depths re-trigger the
    # ConvertTo-Json newline cliff documented above.
    $rows | Export-Csv -NoTypeInformation -Encoding UTF8 -LiteralPath $csvPath
    $rows | ConvertTo-Json -Depth 2 | Set-Content -Encoding UTF8 -LiteralPath $jsonPath

    $summary = Get-BenchmarkSummary -Rows $rows.ToArray()
    [pscustomobject]@{
        run_id = $runId
        rows = $rows.Count
        csv = $csvPath
        json = $jsonPath
        completed = $summary.completed
        failed = $summary.failed
        timed_out = $summary.timed_out
        cancelled = $summary.cancelled
        submit_failed = $summary.submit_failed
        redispatched = $summary.redispatched
        p95_submit_latency_ms = $summary.p95_submit_latency_ms
        p95_terminal_latency_ms = $summary.p95_terminal_latency_ms
    } | ConvertTo-Json -Depth 2
}

if ($MyInvocation.InvocationName -ne ".") {
    Invoke-HivemindSmokeBenchmark
}
