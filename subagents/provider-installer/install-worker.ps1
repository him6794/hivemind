param(
    [Parameter(Mandatory = $true)]
    [string]$MasterUrl,
    [Parameter(Mandatory = $true)]
    [string]$AuthToken,
    [string]$InstallDir = "C:\hivemind-worker",
    [int]$MaxCpuPercent = 80,
    [int]$MaxMemoryMb = 4096,
    [int]$MaxConcurrentTasks = 2
)

$ErrorActionPreference = "Stop"

Write-Host "Installing Hivemind worker to $InstallDir"
$binDir = Join-Path $InstallDir "bin"
$cfgDir = Join-Path $InstallDir "config"
$logDir = Join-Path $InstallDir "logs"

New-Item -ItemType Directory -Force -Path $binDir, $cfgDir, $logDir | Out-Null

$envFile = Join-Path $cfgDir "worker.env"
@(
    "MASTER_HTTP_ADDR=$MasterUrl"
    "WORKER_AUTH_TOKEN=$AuthToken"
    "EXECUTOR_MAX_CPU_PERCENT=$MaxCpuPercent"
    "EXECUTOR_MAX_MEMORY_MB=$MaxMemoryMb"
    "EXECUTOR_MAX_CONCURRENT_TASKS=$MaxConcurrentTasks"
    "EXECUTOR_SANDBOX_MODE=production"
    "EXECUTOR_NETWORK_EGRESS_ENABLED=true"
    "EXECUTOR_NETWORK_EGRESS_MODE=allowlist"
    "EXECUTOR_NETWORK_EGRESS_TARGETS=8.8.8.8,1.1.1.1"
) | Set-Content -Encoding ASCII -Path $envFile

$versionFile = Join-Path $InstallDir "release\version.txt"
$exeSource = Join-Path $InstallDir "release\worker-executor.exe"
$exeTarget = Join-Path $binDir "worker-executor.exe"

if (Test-Path $exeSource) {
    Copy-Item -Force $exeSource $exeTarget
} else {
    Write-Warning "Missing release artifact: $exeSource"
}

if (Test-Path $versionFile) {
    Write-Host ("Installed version: " + (Get-Content $versionFile -Raw).Trim())
} else {
    Write-Host "Installed version: unknown (version.txt missing)"
}

Write-Host "Worker scaffold installed."
