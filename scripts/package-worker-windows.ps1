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
$packagedBinary = Join-Path $out "hivemind-bin.exe"
Copy-Item -Force $binary $packagedBinary

$envTemplate = @"
# Hivemind Windows worker configuration
NODEPOOL_GRPC_ADDR=$NodepoolGrpcAddr
WORKER_GRPC_ADDR=$WorkerGrpcAddr
WORKER_CONTROL_HTTP_ADDR=$WorkerControlHttpAddr
WORKER_ADVERTISE_ADDR=
WORKER_ID=$env:COMPUTERNAME
WORKER_LOCATION=windows

JWT_SECRET=
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

function Import-DotEnv {
    param([Parameter(Mandatory = $true)][string]$Path)

    $seen = @{}
    $lineNumber = 0
    foreach ($rawLine in Get-Content -LiteralPath $Path) {
        $lineNumber += 1
        $trimmed = $rawLine.Trim()
        if ($trimmed -eq "" -or $trimmed.StartsWith("#")) {
            continue
        }

        if ($rawLine -notmatch '^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$') {
            throw "Invalid .env.worker line ${lineNumber}: expected KEY=VALUE with a valid environment variable name."
        }

        $key = $matches[1]
        $value = $matches[2].Trim()
        if ($seen.ContainsKey($key)) {
            throw "Duplicate .env.worker key '$key' on line ${lineNumber}."
        }

        if ($value.Length -ge 2) {
            $first = $value.Substring(0, 1)
            $last = $value.Substring($value.Length - 1, 1)
            if (($first -eq '"' -and $last -eq '"') -or ($first -eq "'" -and $last -eq "'")) {
                $value = $value.Substring(1, $value.Length - 2)
            }
        }

        [Environment]::SetEnvironmentVariable($key, $value, "Process")
        $seen[$key] = $true
    }
}

function Assert-RequiredEnv {
    param([Parameter(Mandatory = $true)][string[]]$Names)

    foreach ($name in $Names) {
        $value = [Environment]::GetEnvironmentVariable($name, "Process")
        if ([string]::IsNullOrWhiteSpace($value)) {
            throw "Required setting $name is missing or blank in .env.worker."
        }
    }

    $jwtSecret = [Environment]::GetEnvironmentVariable("JWT_SECRET", "Process")
    if ($jwtSecret.Trim().Equals("CHANGE_ME_IN_PRODUCTION", [StringComparison]::OrdinalIgnoreCase) -or
        $jwtSecret.Trim().Equals("change-me-in-production", [StringComparison]::OrdinalIgnoreCase)) {
        throw "JWT_SECRET must be set to a non-default deployment secret."
    }
}

$envFile = Join-Path $PSScriptRoot ".env.worker"
if (!(Test-Path $envFile)) {
    Copy-Item (Join-Path $PSScriptRoot ".env.worker.example") $envFile
    Write-Host "Created .env.worker from template. Edit NODEPOOL_GRPC_ADDR, WORKER_ADVERTISE_ADDR, and JWT_SECRET, then rerun."
    exit 1
}

Import-DotEnv -Path $envFile
Assert-RequiredEnv -Names @("NODEPOOL_GRPC_ADDR", "WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR", "WORKER_ADVERTISE_ADDR", "JWT_SECRET")

& (Join-Path $PSScriptRoot "hivemind-bin.exe") worker
'@
$launcher | Set-Content -Encoding ASCII (Join-Path $out "start-worker.ps1")

$readme = @"
# Hivemind Windows Worker Package

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ADDR` to the reachable nodepool gRPC address.
3. Set `WORKER_ADVERTISE_ADDR` to the address other machines can use to reach this worker, for example `203.0.113.10:50053` or a Tailscale address.
4. Set `JWT_SECRET` to the deployment secret issued by the operator. Do not use a default or public placeholder value.
5. Put `monty.exe` next to `hivemind-bin.exe` or update `MONTY_EXECUTABLE`.
6. Run PowerShell as the provider user and execute:

```powershell
.\start-worker.ps1
```

The worker starts its gRPC server, local control API, hardware profile reporting, and nodepool registration loop.
"@
$readme | Set-Content -Encoding ASCII (Join-Path $out "README.md")

$shaFile = Join-Path $out "SHA256SUMS"
$manifestFile = Join-Path $out "manifest.json"
$artifactHash = (Get-FileHash -LiteralPath $packagedBinary -Algorithm SHA256).Hash.ToLowerInvariant()
$gitCommit = try { (git -C $repoRoot rev-parse HEAD 2>$null).Trim() } catch { "unknown" }
$gitDirty = $true
try {
    $gitDirty = -not [string]::IsNullOrWhiteSpace((git -C $repoRoot status --porcelain 2>$null))
} catch {
    $gitDirty = $true
}

("{0} *{1}" -f $artifactHash, "hivemind-bin.exe") | Set-Content -Encoding ASCII -Path $shaFile

$manifest = [ordered]@{
    package = "hivemind-windows-worker"
    configuration = $Configuration
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    git_commit = $gitCommit
    git_dirty = $gitDirty
    artifacts = @(
        [ordered]@{
            name = "hivemind-bin.exe"
            sha256 = $artifactHash
            source = $binary
        }
    )
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Encoding ASCII -Path $manifestFile

Write-Host "Windows worker package written to $out"
