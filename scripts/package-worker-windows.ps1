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
        cargo build --release --no-default-features --features worker --bin hivemind-worker
        $binary = Join-Path $rustRoot "target\release\hivemind-worker.exe"
    } else {
        cargo build --no-default-features --features worker --bin hivemind-worker
        $binary = Join-Path $rustRoot "target\debug\hivemind-worker.exe"
    }
} finally {
    Pop-Location
}

if (!(Test-Path $binary)) {
    throw "Built binary not found: $binary"
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
$packagedBinary = Join-Path $out "hivemind-worker.exe"
Copy-Item -Force $binary $packagedBinary
# Package Worker UI assets served by the worker control HTTP endpoint.
$uiSource = Join-Path $repoRoot "frontend\worker-ui\dist"
$uiDest = Join-Path $out "ui\worker"
if (!(Test-Path -LiteralPath $uiSource -PathType Container)) {
    throw "Worker UI dist was not found at $uiSource. Run npm run build in frontend/worker-ui first."
}
New-Item -ItemType Directory -Force -Path $uiDest | Out-Null
Copy-Item -Recurse -Force -Path (Join-Path $uiSource "*") -Destination $uiDest


$envTemplate = @"
# Hivemind Windows worker configuration
NODEPOOL_GRPC_ADDR=$NodepoolGrpcAddr
WORKER_GRPC_ADDR=$WorkerGrpcAddr
WORKER_CONTROL_HTTP_ADDR=$WorkerControlHttpAddr
WORKER_UI_DIR=.\ui\worker
# Must be reachable by the nodepool; set this explicitly for multi-host deployments.
WORKER_ADVERTISE_ADDR=
WORKER_NODEPOOL_TOKEN=
WORKER_NODEPOOL_USERNAME=
WORKER_NODEPOOL_PASSWORD=
WORKER_ID=$env:COMPUTERNAME
WORKER_LOCATION=windows

# Must exactly match the non-default worker-execution secret used by the nodepool.
WORKER_EXECUTION_SECRET=
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
TORRENT_ALLOW_LOCAL_TASK_ARTIFACTS=false
TORRENT_TASK_ARTIFACT_BASE_URL=
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

    $workerExecutionSecret = [Environment]::GetEnvironmentVariable("WORKER_EXECUTION_SECRET", "Process")
    if ($workerExecutionSecret.Trim().Equals("CHANGE_ME_WORKER_EXECUTION_SECRET", [StringComparison]::OrdinalIgnoreCase) -or
        $workerExecutionSecret.Trim().Equals("change-me-worker-execution-secret", [StringComparison]::OrdinalIgnoreCase) -or
        $workerExecutionSecret.Trim().Equals("CHANGE_ME_IN_PRODUCTION", [StringComparison]::OrdinalIgnoreCase) -or
        $workerExecutionSecret.Trim().Equals("change-me-in-production", [StringComparison]::OrdinalIgnoreCase)) {
        throw "WORKER_EXECUTION_SECRET must be set to a non-default deployment secret."
    }
}

function Reset-CurrentConsoleOpacity {
    $source = @(
        "using System;",
        "using System.Runtime.InteropServices;",
        "public static class ConsoleOpacityReset {",
        "  [DllImport(""kernel32.dll"")] public static extern IntPtr GetConsoleWindow();",
        "  [DllImport(""user32.dll"", SetLastError = true)] public static extern bool SetLayeredWindowAttributes(IntPtr hwnd, uint crKey, byte bAlpha, uint dwFlags);",
        "}"
    ) -join "`r`n"

    try {
        Add-Type -TypeDefinition $source -ErrorAction Stop
        $consoleWindow = [ConsoleOpacityReset]::GetConsoleWindow()
        if ($consoleWindow -ne [IntPtr]::Zero) {
            [ConsoleOpacityReset]::SetLayeredWindowAttributes($consoleWindow, 0, 255, 0x2) | Out-Null
        }
    } catch {
        Write-Warning "Could not reset current console opacity: $($_.Exception.Message)"
    }
}

function Reset-CmdConsoleOpacity {
    $consoleRoot = "HKCU:\Console"
    if (!(Test-Path $consoleRoot)) {
        return
    }

    $keys = @($consoleRoot)
    $keys += Get-ChildItem -LiteralPath $consoleRoot -Recurse -ErrorAction SilentlyContinue |
        ForEach-Object { $_.PSPath }

    foreach ($key in $keys) {
        try {
            Remove-ItemProperty -LiteralPath $key -Name "WindowAlpha" -ErrorAction SilentlyContinue
            Remove-ItemProperty -LiteralPath $key -Name "WindowTransparency" -ErrorAction SilentlyContinue
            New-ItemProperty -LiteralPath $key -Name "WindowAlpha" -Value 255 -PropertyType DWord -Force | Out-Null
        } catch {
            Write-Warning "Could not reset console opacity at ${key}: $($_.Exception.Message)"
        }
    }
}

function Set-JsonProperty {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)]$Value
    )

    if ($Object.PSObject.Properties.Name -contains $Name) {
        $Object.$Name = $Value
    } else {
        Add-Member -InputObject $Object -NotePropertyName $Name -NotePropertyValue $Value
    }
}

function Set-WindowsTerminalProfileOpaque {
    param([Parameter(Mandatory = $true)]$Profile)

    Set-JsonProperty -Object $Profile -Name "useAcrylic" -Value $false
    Set-JsonProperty -Object $Profile -Name "opacity" -Value 100
    Set-JsonProperty -Object $Profile -Name "acrylicOpacity" -Value 1.0
}

function Test-WindowsTerminalCmdProfile {
    param([Parameter(Mandatory = $true)]$Profile)

    $name = [string]$Profile.name
    $commandLine = [string]$Profile.commandline
    if ([string]::IsNullOrWhiteSpace($commandLine)) {
        $commandLine = [string]$Profile.commandLine
    }

    return $commandLine -match '(?i)(^|\\)cmd\.exe($|\s)' -or
        $name.Equals("Command Prompt", [StringComparison]::OrdinalIgnoreCase) -or
        $name.Equals("命令提示字元", [StringComparison]::OrdinalIgnoreCase)
}

function Reset-WindowsTerminalCmdOpacity {
    $settingsPaths = @(
        (Join-Path $env:LOCALAPPDATA "Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json"),
        (Join-Path $env:LOCALAPPDATA "Packages\Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe\LocalState\settings.json"),
        (Join-Path $env:LOCALAPPDATA "Microsoft\Windows Terminal\settings.json")
    )

    foreach ($settingsPath in $settingsPaths) {
        if (!(Test-Path -LiteralPath $settingsPath)) {
            continue
        }

        try {
            $settings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
            if ($null -eq $settings.profiles) {
                continue
            }

            if ($null -eq $settings.profiles.defaults) {
                Set-JsonProperty -Object $settings.profiles -Name "defaults" -Value ([pscustomobject]@{})
            }

            Set-JsonProperty -Object $settings -Name "useAcrylicInTabRow" -Value $false
            Set-WindowsTerminalProfileOpaque -Profile $settings.profiles.defaults

            foreach ($profile in @($settings.profiles.list)) {
                if ($null -ne $profile -and (Test-WindowsTerminalCmdProfile -Profile $profile)) {
                    Set-WindowsTerminalProfileOpaque -Profile $profile
                }
            }

            $settings | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $settingsPath -Encoding UTF8
        } catch {
            Write-Warning "Could not reset Windows Terminal opacity at ${settingsPath}: $($_.Exception.Message)"
        }
    }
}

Reset-CmdConsoleOpacity
Reset-WindowsTerminalCmdOpacity
Reset-CurrentConsoleOpacity

$envFile = Join-Path $PSScriptRoot ".env.worker"
if (!(Test-Path $envFile)) {
    Copy-Item (Join-Path $PSScriptRoot ".env.worker.example") $envFile
    Write-Host "Created .env.worker from template."
}

Import-DotEnv -Path $envFile
Assert-RequiredEnv -Names @("NODEPOOL_GRPC_ADDR", "WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR", "WORKER_ADVERTISE_ADDR", "WORKER_EXECUTION_SECRET")

$workerNodepoolToken = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_TOKEN", "Process")
$workerNodepoolUsername = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_USERNAME", "Process")
$workerNodepoolPassword = [Environment]::GetEnvironmentVariable("WORKER_NODEPOOL_PASSWORD", "Process")
if ([string]::IsNullOrWhiteSpace($workerNodepoolToken) -and
    ([string]::IsNullOrWhiteSpace($workerNodepoolUsername) -or [string]::IsNullOrWhiteSpace($workerNodepoolPassword))) {
    throw "Set WORKER_NODEPOOL_TOKEN, or set both WORKER_NODEPOOL_USERNAME and WORKER_NODEPOOL_PASSWORD."
}

& (Join-Path $PSScriptRoot "hivemind-worker.exe")
'@
$launcher | Set-Content -Encoding ASCII (Join-Path $out "start-worker.ps1")

$readme = @"
# Hivemind Windows Worker Package

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ADDR` to the reachable nodepool gRPC address.
3. Set `WORKER_NODEPOOL_TOKEN` to a nodepool JWT whose subject matches `WORKER_ID`, or set both `WORKER_NODEPOOL_USERNAME` and `WORKER_NODEPOOL_PASSWORD` so the worker can log in to nodepool.
4. Set `WORKER_ADVERTISE_ADDR` to the address other machines can use to reach this worker, for example `203.0.113.10:50053` or a Tailscale address. It is required when `WORKER_GRPC_ADDR` listens on `0.0.0.0` (the package default).
5. Set `WORKER_EXECUTION_SECRET` to the same non-default worker-execution secret used by the nodepool. Do not provide the control-plane `JWT_SECRET` to workers.
6. Put `monty.exe` next to `hivemind-worker.exe` or update `MONTY_EXECUTABLE`.
7. Run PowerShell as the provider user and execute:

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

("{0} *{1}" -f $artifactHash, "hivemind-worker.exe") | Set-Content -Encoding ASCII -Path $shaFile

$manifest = [ordered]@{
    package = "hivemind-windows-worker"
    configuration = $Configuration
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    git_commit = $gitCommit
    git_dirty = $gitDirty
    artifacts = @(
        [ordered]@{
            name = "hivemind-worker.exe"
            sha256 = $artifactHash
            source = $binary
        }
    )
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Encoding ASCII -Path $manifestFile

Write-Host "Windows worker package written to $out"



