$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "hivemind-smoke-benchmark.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "smoke benchmark script must exist"
}

. $scriptPath `
    -MasterUrl "http://127.0.0.1" `
    -Token "offline-test-token" `
    -TaskSourcePath "unused-in-offline-tests.hmf"

function Assert-Equal {
    param(
        [AllowNull()][object]$Expected,
        [AllowNull()][object]$Actual,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if ($null -eq $Expected) {
        if ($null -ne $Actual) { throw "$Message Expected <null>, got <$Actual>." }
        return
    }
    if ($null -eq $Actual -or $Expected -ne $Actual) {
        throw "$Message Expected <$Expected>, got <$Actual>."
    }
}

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (!$Condition) { throw $Message }
}

function New-TestBenchmarkRow {
    param([string]$TaskId = "test-task")

    [pscustomobject]@{
        task_id = $TaskId
        status = "SUBMITTED"
        submit_latency_ms = $null
        terminal_latency_ms = $null
        wait_duration_ms = $null
        redispatched = $null
        wall_time_ms = $null
        peak_memory_mb = $null
        message = $null
    }
}

Assert-Equal -Expected "hcode" -Actual $Runtime -Message "The benchmark must default to the active V1 runtime."
Assert-Equal -Expected 3 -Actual $WorkerCounts[0] -Message "The first default scenario must require three workers."
Assert-Equal -Expected 5 -Actual $WorkerCounts[1] -Message "The second default scenario must require five workers."
Assert-Equal -Expected 10 -Actual $TaskCounts[0] -Message "The first default scenario must submit ten tasks."
Assert-Equal -Expected 100 -Actual $TaskCounts[1] -Message "The second default scenario must submit one hundred tasks."

$accepted = Start-BenchmarkTaskSubmission -TaskId "accepted-task" -SubmitAction {
    param($taskId, $clock)
    if ($taskId -ne "accepted-task") { throw "submission received an unexpected task id" }
    if (!$clock.IsRunning) { throw "task stopwatch must start before the submission action" }
    [pscustomobject]@{ success = $true; message = "accepted" }
}
Assert-Equal -Expected "SUBMITTED" -Actual $accepted.status -Message "Successful submission must be pending."
Assert-True -Condition ($accepted.submit_latency_ms -ge 0) -Message "Successful submission must report a measured submit duration."

$rejected = Start-BenchmarkTaskSubmission -TaskId "rejected-task" -SubmitAction {
    param($taskId, $clock)
    if (!$clock.IsRunning) { throw "task stopwatch must start before a rejected submission" }
    [pscustomobject]@{ success = $false; message = "rejected" }
}
Assert-Equal -Expected "SUBMIT_FAILED" -Actual $rejected.status -Message "A rejected POST must be marked SUBMIT_FAILED."
Assert-Equal -Expected "rejected" -Actual $rejected.message -Message "The submit failure message must be retained."
Assert-True -Condition (!$rejected.clock.IsRunning) -Message "A failed submission must stop its task stopwatch."

$transportFailure = Start-BenchmarkTaskSubmission -TaskId "transport-failure" -SubmitAction {
    param($taskId, $clock)
    if (!$clock.IsRunning) { throw "task stopwatch must start before a transport failure" }
    throw "offline transport failure"
}
Assert-Equal -Expected "SUBMIT_FAILED" -Actual $transportFailure.status -Message "A submit exception must be marked SUBMIT_FAILED."
Assert-Equal -Expected "offline transport failure" -Actual $transportFailure.message -Message "A submit exception message must be retained."

$pending = [string[]]@(Get-BenchmarkPendingTaskIds -Rows @(
    [pscustomobject]@{ task_id = "accepted-task"; status = $accepted.status }
    [pscustomobject]@{ task_id = "rejected-task"; status = $rejected.status }
    [pscustomobject]@{ task_id = "transport-failure"; status = $transportFailure.status }
))
Assert-Equal -Expected 1 -Actual $pending.Count -Message "Only accepted submissions may enter the pending set."
Assert-Equal -Expected "accepted-task" -Actual $pending[0] -Message "The pending set must contain the accepted task only."
$accepted.clock.Stop()

$completedRow = New-TestBenchmarkRow -TaskId "completed-task"
$completed = Set-BenchmarkTaskTerminalOutcome `
    -Row $completedRow `
    -Task ([pscustomobject]@{
        status = "completed"
        retry_count = 0
        wall_time_ms = 0
        peak_memory_mb = 0
        status_message = "done"
    }) `
    -ElapsedMs 1234
Assert-True -Condition $completed -Message "COMPLETED must be recognized as terminal."
Assert-Equal -Expected "COMPLETED" -Actual $completedRow.status -Message "Terminal status must be normalized."
Assert-Equal -Expected 1234 -Actual $completedRow.terminal_latency_ms -Message "Terminal latency must preserve the supplied per-task elapsed duration."
Assert-Equal -Expected $null -Actual $completedRow.wait_duration_ms -Message "A terminal result is not a censored wait."
Assert-Equal -Expected 0 -Actual $completedRow.redispatched -Message "A measured zero retry count must remain zero."
Assert-Equal -Expected 0 -Actual $completedRow.wall_time_ms -Message "A measured zero wall time must remain zero."
Assert-Equal -Expected 0 -Actual $completedRow.peak_memory_mb -Message "A measured zero peak-memory value must remain zero."

$unknownRow = New-TestBenchmarkRow -TaskId "unknown-metrics-task"
[void](Set-BenchmarkTaskTerminalOutcome `
    -Row $unknownRow `
    -Task ([pscustomobject]@{ status = "COMPLETED" }) `
    -ElapsedMs 4321)
Assert-Equal -Expected $null -Actual $unknownRow.redispatched -Message "Missing retry count must remain unknown."
Assert-Equal -Expected $null -Actual $unknownRow.wall_time_ms -Message "Missing wall time must remain unknown."
Assert-Equal -Expected $null -Actual $unknownRow.peak_memory_mb -Message "Missing peak memory must remain unknown."
Assert-Equal -Expected $null -Actual $unknownRow.message -Message "Missing status message must remain unknown."

$apiTimedOutRow = New-TestBenchmarkRow -TaskId "api-timeout-task"
$apiTimedOut = Set-BenchmarkTaskTerminalOutcome `
    -Row $apiTimedOutRow `
    -Task ([pscustomobject]@{ status = "TIMED_OUT" }) `
    -ElapsedMs 5000
Assert-True -Condition $apiTimedOut -Message "TIMED_OUT from the API must be recognized as terminal."
Assert-Equal -Expected "TIMED_OUT" -Actual $apiTimedOutRow.status -Message "The actual terminal TIMED_OUT status must be retained."

$censoredRow = New-TestBenchmarkRow -TaskId "censored-task"
Set-BenchmarkTaskCensoredOutcome -Row $censoredRow -WaitDurationMs 60000
Assert-Equal -Expected "TIMEOUT_WAITING_FOR_TERMINAL_STATUS" -Actual $censoredRow.status -Message "A local wait timeout must be recorded as a censored outcome."
Assert-Equal -Expected $null -Actual $censoredRow.terminal_latency_ms -Message "A censored outcome must not claim completed terminal latency."
Assert-Equal -Expected 60000 -Actual $censoredRow.wait_duration_ms -Message "A censored outcome must retain its observed wait duration."

$summaryRows = @(
    [pscustomobject]@{ status = "COMPLETED"; submit_latency_ms = 100; terminal_latency_ms = 10; redispatched = 0 }
    [pscustomobject]@{ status = "COMPLETED"; submit_latency_ms = 200; terminal_latency_ms = 20; redispatched = $null }
    [pscustomobject]@{ status = "FAILED"; submit_latency_ms = 300; terminal_latency_ms = 999; redispatched = $null }
    [pscustomobject]@{ status = "TIMED_OUT"; submit_latency_ms = 400; terminal_latency_ms = 888; redispatched = $null }
    [pscustomobject]@{ status = "CANCELLED"; submit_latency_ms = 500; terminal_latency_ms = 777; redispatched = $null }
    [pscustomobject]@{ status = "TIMEOUT_WAITING_FOR_TERMINAL_STATUS"; submit_latency_ms = 600; terminal_latency_ms = $null; wait_duration_ms = 60000; redispatched = $null }
    [pscustomobject]@{ status = "SUBMIT_FAILED"; submit_latency_ms = 700; terminal_latency_ms = $null; redispatched = $null }
    [pscustomobject]@{ status = "COMPLETED"; submit_latency_ms = $null; terminal_latency_ms = $null; redispatched = $null }
)
$summary = Get-BenchmarkSummary -Rows $summaryRows
Assert-Equal -Expected 3 -Actual $summary.completed -Message "Summary must count only completed outcomes."
Assert-Equal -Expected 1 -Actual $summary.failed -Message "Summary must count FAILED separately."
Assert-Equal -Expected 2 -Actual $summary.timed_out -Message "Summary must count both API and local wait timeouts."
Assert-Equal -Expected 1 -Actual $summary.cancelled -Message "Summary must count CANCELLED separately."
Assert-Equal -Expected 1 -Actual $summary.submit_failed -Message "Summary must count failed submissions separately."
Assert-Equal -Expected 0 -Actual $summary.redispatched -Message "Known zero redispatch must remain zero even when other counts are unknown."
Assert-Equal -Expected 600 -Actual $summary.p95_submit_latency_ms -Message "Submit p95 must use known successful submission samples and exclude submission failures."
Assert-Equal -Expected 20 -Actual $summary.p95_terminal_latency_ms -Message "Terminal p95 must include only completed tasks with known latency."

$unknownSummary = Get-BenchmarkSummary -Rows @(
    [pscustomobject]@{ status = "COMPLETED"; submit_latency_ms = $null; terminal_latency_ms = $null; redispatched = $null }
)
Assert-Equal -Expected $null -Actual $unknownSummary.redispatched -Message "All-unknown redispatch totals must remain null."
Assert-Equal -Expected $null -Actual $unknownSummary.p95_submit_latency_ms -Message "An empty known submit sample must produce a null percentile."
Assert-Equal -Expected $null -Actual $unknownSummary.p95_terminal_latency_ms -Message "An empty completed-latency sample must produce a null percentile."

$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("hivemind-smoke-benchmark-test-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$testSourcePath = Join-Path $testRoot "task.hmf"
Set-Content -LiteralPath $testSourcePath -Value "offline test source" -Encoding UTF8
$script:MockSubmittedTaskIds = New-Object System.Collections.Generic.List[string]
$script:MockTaskListRequests = 0

function Invoke-RestMethod {
    param(
        [Parameter(Mandatory = $true)][string]$Method,
        [Parameter(Mandatory = $true)][string]$Uri,
        [hashtable]$Headers,
        [string]$ContentType,
        [string]$Body
    )

    if ($Method -eq "Get" -and $Uri.EndsWith("/api/workers")) {
        return [pscustomobject]@{ workers = @("worker-1", "worker-2", "worker-3") }
    }
    if ($Method -eq "Get" -and $Uri.EndsWith("/api/tasks")) {
        $script:MockTaskListRequests++
        return [pscustomobject]@{
            tasks = @(
                [pscustomobject]@{
                    task_id = $script:MockSubmittedTaskIds[0]
                    status = "COMPLETED"
                    retry_count = 0
                    peak_memory_mb = 0
                    status_message = "done"
                }
                [pscustomobject]@{
                    task_id = $script:MockSubmittedTaskIds[2]
                    status = "TIMED_OUT"
                }
            )
        }
    }
    if ($Method -eq "Post" -and $Uri.EndsWith("/api/tasks")) {
        $submittedTask = $Body | ConvertFrom-Json
        $script:MockSubmittedTaskIds.Add([string]$submittedTask.task_id)
        if ($script:MockSubmittedTaskIds.Count -eq 2) {
            return [pscustomobject]@{ success = $false; message = "rejected" }
        }
        return [pscustomobject]@{ success = $true; message = "accepted" }
    }
    throw "Unexpected offline HTTP request: $Method $Uri"
}

try {
    $MasterUrl = "http://offline.test"
    $TaskSourcePath = $testSourcePath
    $WorkerCounts = @(3)
    $TaskCounts = @(4)
    $PollSeconds = 0
    $TimeoutSeconds = 0
    $OutputDir = Join-Path $testRoot "output"

    $actualSummary = Invoke-HivemindSmokeBenchmark | ConvertFrom-Json
    Assert-Equal -Expected 4 -Actual $actualSummary.rows -Message "Offline run must write a row for each submission attempt."
    Assert-Equal -Expected 1 -Actual $actualSummary.completed -Message "Offline run must classify completed tasks."
    Assert-Equal -Expected 2 -Actual $actualSummary.timed_out -Message "Offline run must count API and censored timeouts."
    Assert-Equal -Expected 1 -Actual $actualSummary.submit_failed -Message "Offline run must not convert a failed submission into a pending timeout."
    Assert-Equal -Expected 1 -Actual $script:MockTaskListRequests -Message "Failed submission must not trigger additional pending polling."

    # Windows PowerShell 5.1's ConvertFrom-Json can return the parsed array
    # wrapped as a single-element collection; unwrap it so the report reads as
    # one row per task.
    $reportRows = @(Get-Content -LiteralPath $actualSummary.json -Raw | ConvertFrom-Json)
    if ($reportRows.Count -eq 1 -and $reportRows[0] -is [System.Collections.IList]) {
        $reportRows = @($reportRows[0])
    }
    $completedReportRow = @($reportRows | Where-Object { $_.status -eq "COMPLETED" })[0]
    Assert-Equal -Expected 0 -Actual $completedReportRow.redispatched -Message "Export must preserve explicit zero retry count."
    Assert-Equal -Expected $null -Actual $completedReportRow.wall_time_ms -Message "Export must preserve unknown wall time as null."
    Assert-Equal -Expected 0 -Actual $completedReportRow.peak_memory_mb -Message "Export must preserve explicit zero peak memory."
    $apiTimeoutReportRow = @($reportRows | Where-Object { $_.status -eq "TIMED_OUT" })[0]
    Assert-True -Condition ($null -ne $apiTimeoutReportRow.terminal_latency_ms) -Message "API TIMED_OUT must retain its observed terminal latency."
    $censoredReportRow = @($reportRows | Where-Object { $_.status -eq "TIMEOUT_WAITING_FOR_TERMINAL_STATUS" })[0]
    Assert-Equal -Expected $null -Actual $censoredReportRow.terminal_latency_ms -Message "Local wait timeout must not report terminal latency."
    Assert-True -Condition ($null -ne $censoredReportRow.wait_duration_ms) -Message "Local wait timeout must report its censored wait duration."
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force -Confirm:$false
}

Write-Host "hivemind smoke benchmark tests passed"
