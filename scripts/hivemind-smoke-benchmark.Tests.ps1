$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "hivemind-smoke-benchmark.ps1"
if (!(Test-Path $scriptPath)) {
    throw "smoke benchmark script must exist"
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw

function Assert-Contains {
    param(
        [Parameter(Mandatory = $true)][string]$Haystack,
        [Parameter(Mandatory = $true)][string]$Needle,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (!$Haystack.Contains($Needle)) {
        throw $Message
    }
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WorkerCounts = @(1, 5)" `
    -Message "benchmark must cover the recommended 1-worker and 5-worker smoke sizes."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "TaskCounts = @(10, 100)" `
    -Message "benchmark must cover the recommended 10-task and 100-task smoke sizes."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "submit_latency_ms" `
    -Message "benchmark output must include submit latency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "terminal_latency_ms" `
    -Message "benchmark output must include terminal task latency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "redispatched" `
    -Message "benchmark output must include redispatch counts."

Write-Host "hivemind smoke benchmark tests passed"
