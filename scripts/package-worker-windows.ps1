param(
    [string]$Configuration = "release",
    [string]$OutputDir = "dist\windows-worker",
    [string]$NodepoolGrpcAddr = "nodepool.example.com:50051",
    [string]$WorkerGrpcAddr = "0.0.0.0:50053",
    [string]$WorkerControlHttpAddr = "127.0.0.1:18080"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$rustRoot = Join-Path $repoRoot "hivemind-rs"
$out = Join-Path $repoRoot $OutputDir

if ($Configuration -ne "release" -and $Configuration -ne "debug") {
    throw "Configuration must be 'release' or 'debug'."
}

Push-Location $rustRoot
try {
    if ($Configuration -eq "release") {
        cargo build --release --bin hivemind-bin
        $binary = Join-Path $rustRoot "target\release\hivemind-bin.exe"
    } else {
        cargo build --bin hivemind-bin
        $binary = Join-Path $rustRoot "target\debug\hivemind-bin.exe"
    }
} finally {
    Pop-Location
}

if (!(Test-Path $binary)) {
    throw "Built binary not found: $binary"
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item -Force $binary (Join-Path $out "hivemind-bin.exe")

$envTemplate = @"
# Hivemind Windows worker configuration
NODEPOOL_GRPC_ADDR=$NodepoolGrpcAddr
WORKER_GRPC_ADDR=$WorkerGrpcAddr
WORKER_CONTROL_HTTP_ADDR=$WorkerControlHttpAddr
WORKER_ADVERTISE_ADDR=
WORKER_ID=$env:COMPUTERNAME
WORKER_LOCATION=windows

JWT_SECRET=change-me-in-production
MONTY_EXECUTABLE=monty.exe
EXECUTOR_SANDBOX_DIR=.\sandbox
EXECUTOR_MAX_CPU_PERCENT=80
EXECUTOR_MAX_MEMORY_MB=4096
EXECUTOR_TASK_TIMEOUT_SECS=3600
EXECUTOR_MAX_CONCURRENT_TASKS=2
EXECUTOR_SANDBOX_MODE=production
EXECUTOR_NETWORK_EGRESS_ENABLED=true
EXECUTOR_NETWORK_EGRESS_MODE=allowlist
EXECUTOR_NETWORK_EGRESS_TARGETS=127.0.0.1
"@
$envTemplate | Set-Content -Encoding ASCII (Join-Path $out ".env.worker.example")

$launcher = @'
$ErrorActionPreference = "Stop"
$envFile = Join-Path $PSScriptRoot ".env.worker"
if (!(Test-Path $envFile)) {
    Copy-Item (Join-Path $PSScriptRoot ".env.worker.example") $envFile
    Write-Host "Created .env.worker from template. Edit NODEPOOL_GRPC_ADDR and WORKER_ADVERTISE_ADDR, then rerun."
    exit 1
}

Get-Content $envFile | ForEach-Object {
    $line = $_.Trim()
    if ($line -eq "" -or $line.StartsWith("#")) { return }
    $parts = $line.Split("=", 2)
    if ($parts.Length -eq 2) {
        [Environment]::SetEnvironmentVariable($parts[0], $parts[1], "Process")
    }
}

& (Join-Path $PSScriptRoot "hivemind-bin.exe") worker
'@
$launcher | Set-Content -Encoding ASCII (Join-Path $out "start-worker.ps1")

$readme = @"
# Hivemind Windows Worker Package

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ADDR` to the reachable nodepool gRPC address.
3. Set `WORKER_ADVERTISE_ADDR` to the address other machines can use to reach this worker, for example `203.0.113.10:50053` or a Tailscale address.
4. Put `monty.exe` next to `hivemind-bin.exe` or update `MONTY_EXECUTABLE`.
5. Run PowerShell as the provider user and execute:

```powershell
.\start-worker.ps1
```

The worker starts its gRPC server, local control API, hardware profile reporting, and nodepool registration loop.
"@
$readme | Set-Content -Encoding ASCII (Join-Path $out "README.md")

Write-Host "Windows worker package written to $out"
