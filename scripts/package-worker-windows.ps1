param(
    [string]$Configuration = "release",
    [ValidateSet("x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu", "aarch64-pc-windows-msvc")]
    [string]$RustTarget = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "dist\windows-worker",
    [string]$NodepoolGrpcAddr = "nodepool.example.com:50051",
    [string]$NodepoolGrpcEndpoint = "",
    [string]$HeadscaleLoginServer = "",
    [string]$WebsiteApiBase = "",
    [string]$WorkerVpnAuthkey = "",
    [string]$WorkerVpnHostname = "",
    [ValidateRange(1, 300)][int]$VpnStartupTimeoutSecs = 30,
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

$artifactDir = switch ($RustTarget) {
    "x86_64-pc-windows-msvc" { Join-Path $repoRoot "vendor\libtailscale\windows-x86_64-msvc"; break }
    "aarch64-pc-windows-msvc" { Join-Path $repoRoot "vendor\libtailscale\windows-aarch64-msvc"; break }
    default { Join-Path $repoRoot "vendor\libtailscale\windows-x86_64" }
}
$archiveName = if ($RustTarget -eq "x86_64-pc-windows-gnu") { "libtailscale.a" } else { "libtailscale.dll" }
$archive = Join-Path $artifactDir $archiveName
$header = Join-Path $artifactDir "tailscale.h"
if (!(Test-Path -LiteralPath $archive) -or !(Test-Path -LiteralPath $header)) {
    throw "Missing ABI-specific libtailscale artifact for $RustTarget. Expected $archive and $header. Run scripts/fetch_libtailscale_windows.sh with the matching target before packaging."
}
$vcRuntimeSource = $null
if ($RustTarget -like "*-pc-windows-msvc") {
    $runtimeArchitecture = if ($RustTarget.StartsWith("aarch64-")) { "arm64" } else { "x64" }
    $redistRoot = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\2022\BuildTools\VC\Redist\MSVC"
    if (!(Test-Path -LiteralPath $redistRoot)) {
        throw "Visual C++ redistributable directory is required for ${RustTarget}: $redistRoot"
    }
    $vcRuntimeSource = Get-ChildItem -Path $redistRoot -Recurse -File -Filter "vcruntime140.dll" |
        Where-Object {
            $_.FullName -match "\\$runtimeArchitecture\\Microsoft\.VC[0-9]+\.CRT\\vcruntime140\.dll$"
        } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if ($null -eq $vcRuntimeSource) {
        throw "Matching vcruntime140.dll was not found for ${RustTarget} below $redistRoot"
    }
}

Push-Location $rustRoot
try {
    $cargoArgs = @("build", "--locked", "--target", $RustTarget, "--bin", "hivemind-bin")
    if ($Configuration -eq "release") {
        $cargoArgs += "--release"
    }
    if ($RustTarget -like "*-pc-windows-msvc") {
        $vsDevCmd = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
        if (!(Test-Path -LiteralPath $vsDevCmd)) {
            throw "Visual Studio Build Tools VsDevCmd.bat is required for ${RustTarget}: $vsDevCmd"
        }
        $targetArch = if ($RustTarget.StartsWith("aarch64-")) { "arm64" } else { "x64" }
        $llvmBin = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\MartinStorsjo.LLVM-MinGW.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\llvm-mingw-20260616-ucrt-x86_64\bin"
        $cargoCommand = "cargo build --locked --target $RustTarget --bin hivemind-bin"
        if ($Configuration -eq "release") {
            $cargoCommand += " --release"
        }
        $cmdLine = "call `"$vsDevCmd`" -arch=$targetArch -host_arch=x64 && set GOTELEMETRY=off"
        if (Test-Path -LiteralPath (Join-Path $llvmBin "clang.exe")) {
            $cmdLine += " && set PATH=$llvmBin;%PATH%"
        }
        $cmdLine += " && $cargoCommand"
        & cmd.exe /d /s /c $cmdLine
    } else {
        & cargo @cargoArgs
    }
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo failed for target $RustTarget with exit code $LASTEXITCODE."
    }
    $profile = if ($Configuration -eq "release") { "release" } else { "debug" }
    $binary = Join-Path $rustRoot "target\$RustTarget\$profile\hivemind-bin.exe"
} finally {
    Pop-Location
}

if (!(Test-Path $binary)) {
    throw "Built binary not found: $binary"
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
$packagedBinary = Join-Path $out "hivemind-bin.exe"
Copy-Item -Force $binary $packagedBinary
$packageArtifacts = @(
    [ordered]@{
        name = "hivemind-bin.exe"
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedBinary).Hash.ToLowerInvariant()
        source = $binary
    }
)
if ($RustTarget -like "*-pc-windows-msvc") {
    $packagedLibtailscale = Join-Path $out "libtailscale.dll"
    Copy-Item -Force $archive $packagedLibtailscale
    $packageArtifacts += [ordered]@{
        name = "libtailscale.dll"
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedLibtailscale).Hash.ToLowerInvariant()
        source = $archive
    }
    $packagedVcRuntime = Join-Path $out "vcruntime140.dll"
    Copy-Item -Force $vcRuntimeSource.FullName $packagedVcRuntime
    $packageArtifacts += [ordered]@{
        name = "vcruntime140.dll"
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedVcRuntime).Hash.ToLowerInvariant()
        source = $vcRuntimeSource.FullName
    }
} else {
    $packagedVcRuntime = $null
}

$provenance = [ordered]@{
    rustTarget = $RustTarget
    configuration = $Configuration
    libtailscaleArchive = $archiveName
    libtailscaleArchiveSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
    libtailscaleHeaderSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $header).Hash.ToLowerInvariant()
    binarySha256 = $packageArtifacts[0].sha256
    vcruntime140Sha256 = if ($null -ne $packagedVcRuntime) {
        (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedVcRuntime).Hash.ToLowerInvariant()
    } else {
        $null
    }
}
$provenance | ConvertTo-Json | Set-Content -Encoding ASCII (Join-Path $out "native-dependency-provenance.json")

$envTemplate = @"
# Hivemind Windows worker configuration
# NODEPOOL_GRPC_ADDR is retained for local/backward-compatible deployments.
NODEPOOL_GRPC_ADDR=$NodepoolGrpcAddr
NODEPOOL_GRPC_ENDPOINT=$NodepoolGrpcEndpoint
# HTTPS origin of the deployed Rust Website API. It must expose /api/login and /api/vpn/config.
# Leave blank only when using the built-in public default or an explicit role-specific override.
WEBSITE_API_BASE=$WebsiteApiBase
HEADSCALE_LOGIN_SERVER=$HeadscaleLoginServer
WORKER_VPN_AUTHKEY=$WorkerVpnAuthkey
WORKER_VPN_HOSTNAME=$WorkerVpnHostname
VPN_STARTUP_TIMEOUT_SECS=$VpnStartupTimeoutSecs
WORKER_GRPC_ADDR=$WorkerGrpcAddr
WORKER_CONTROL_HTTP_ADDR=$WorkerControlHttpAddr
WORKER_ADVERTISE_ADDR=
WORKER_NODEPOOL_TOKEN=
# Leave blank to use the target machine's COMPUTERNAME.
WORKER_ID=
WORKER_LOCATION=windows

JWT_SECRET=
# Managed-function proving is unsupported on native Windows: RISC Zero proving
# hosts are Linux, macOS, and WSL. This package ships no prover sidecar, so
# MANAGED_PROVER_EXECUTABLE stays unset and managed tasks fail closed here.
# Managed tasks must run on a worker image or runtime that contains the Linux
# prover sidecar.
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
# Operator-owned native Windows HCS backend registry. The worker fails closed
# if this file is set but missing, malformed, or invalid.
HIVEMIND_GENERAL_COMPUTE_WINDOWS_BACKENDS=
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

function New-RandomJwtSecret {
    $bytes = New-Object byte[] 32
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($bytes)
    } finally {
        $rng.Dispose()
    }
    return -join ($bytes | ForEach-Object { $_.ToString("x2") })
}

function Ensure-JwtSecret {
    param([Parameter(Mandatory = $true)][string]$Path)

    $jwtSecret = [Environment]::GetEnvironmentVariable("JWT_SECRET", "Process")
    if (-not [string]::IsNullOrWhiteSpace($jwtSecret) -and
        -not $jwtSecret.Trim().Equals("CHANGE_ME_IN_PRODUCTION", [StringComparison]::OrdinalIgnoreCase) -and
        -not $jwtSecret.Trim().Equals("change-me-in-production", [StringComparison]::OrdinalIgnoreCase)) {
        return
    }

    $jwtSecret = New-RandomJwtSecret
    [Environment]::SetEnvironmentVariable("JWT_SECRET", $jwtSecret, "Process")

    $contents = Get-Content -LiteralPath $Path -Raw
    if ($contents -match '(?m)^JWT_SECRET=.*$') {
        $contents = [regex]::Replace($contents, '(?m)^JWT_SECRET=.*$', "JWT_SECRET=$jwtSecret")
    } else {
        if ($contents.Length -gt 0 -and -not $contents.EndsWith("`n")) {
            $contents += "`r`n"
        }
        $contents += "JWT_SECRET=$jwtSecret`r`n"
    }

    Set-Content -LiteralPath $Path -Value $contents -Encoding ASCII
    Write-Host "Generated a local JWT_SECRET and stored it in .env.worker."
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
Ensure-JwtSecret -Path $envFile
Assert-RequiredEnv -Names @("WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR")
if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("NODEPOOL_GRPC_ENDPOINT", "Process")) -and
    [string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable("NODEPOOL_GRPC_ADDR", "Process"))) {
    throw "Required setting NODEPOOL_GRPC_ENDPOINT or NODEPOOL_GRPC_ADDR is missing or blank in .env.worker."
}

& (Join-Path $PSScriptRoot "hivemind-bin.exe") worker
$workerExitCode = $LASTEXITCODE
if ($workerExitCode -ne 0) {
    exit $workerExitCode
}
'@
$launcher | Set-Content -Encoding ASCII (Join-Path $out "start-worker.ps1")

# Single-quoted here-string on purpose: this text is Markdown, and in a
# double-quoted here-string PowerShell eats every backtick as an escape
# character, stripping the inline code spans and breaking the fenced blocks.
$readme = @'
# Hivemind Windows Worker Package

1. Copy `.env.worker.example` to `.env.worker`.
2. Set `NODEPOOL_GRPC_ENDPOINT` to the nodepool gRPC address reachable through Headscale. `NODEPOOL_GRPC_ADDR` remains available for backward-compatible local deployments.
3. Set `WEBSITE_API_BASE` to the HTTPS origin of the deployed Rust Website API. That origin must expose `POST /api/login` and the protected `POST /api/vpn/config`; the official Next BFF is not a substitute unless it explicitly serves that contract. `WORKER_WEBSITE_API_BASE` can override it for this role.
4. For interactive enrollment, leave `WORKER_VPN_AUTHKEY` blank and sign in through Worker UI. The local worker sends the bearer JWT to the Website API, consumes the returned one-time Headscale key in memory, joins the overlay, and waits for the Nodepool gRPC transport before registration. A valid persisted VPN state is rehydrated first on restart.
5. For unattended operator startup, optionally set `WORKER_VPN_AUTHKEY` to a role-scoped preauth key. `HEADSCALE_LOGIN_SERVER` and `WORKER_VPN_HOSTNAME` are optional overrides; keyed startup fails closed until the VPN and Nodepool transport are ready.
6. Optionally set `WORKER_NODEPOOL_TOKEN` to a nodepool JWT whose subject matches `WORKER_ID`, or set `WORKER_NODEPOOL_USERNAME` and `WORKER_NODEPOOL_PASSWORD`; when these are blank, the local Worker UI can perform registration after startup.
7. Optionally set `WORKER_ADVERTISE_ADDR` to an address other machines can use to reach this worker. When Headscale startup is keyed and the worker listens on `0.0.0.0`, the connected overlay IP is used automatically.
8. `JWT_SECRET` will be generated automatically on first launch if it is blank. Set it explicitly if you need a fixed deployment secret.
9. Run PowerShell as the provider user and execute:

```powershell
.\start-worker.ps1
```

The worker joins Headscale before startup-dependent registration, waits for the Nodepool gRPC transport, then starts its gRPC server, local control API, hardware profile reporting, and registration loop. Without `WORKER_VPN_AUTHKEY`, the local UI remains available while enrollment waits for an authenticated login. If the JWT expires or the device state is revoked, sign in again; no password, `HEADSCALE_API_KEY`, or reusable Headscale key is written to the package or browser storage.

The downloaded Worker runs on the provider's local suitable host. Orange Pi is reserved for Nodepool, Website API, Headscale, PostgreSQL, and Redis; do not deploy this Worker package there.

## Managed proving

Windows workers run ordinary worker workloads, but this package does not include
a RISC Zero prover sidecar: RISC Zero proving hosts are Linux, macOS, and WSL,
and native Windows proving is unsupported. Managed proving therefore fails closed
on this worker - `managed-function-v0` tasks are rejected rather than settled
from unverified numbers. That is the intended safe behaviour, not a silent
downgrade, which is why `MANAGED_PROVER_EXECUTABLE` is left unset here; pointing
it at a Windows path does not make proving work.

Managed tasks must run on a worker image or runtime that contains the Linux
prover sidecar, so managed proving requires deploying this worker on a supported
Linux-based runtime instead. Build that sidecar on a Linux, macOS, or WSL host.
From a Windows checkout of the repository:

```powershell
wsl bash scripts/build-managed-prover.sh
```

Where network policy blocks the RISC Zero artifact bucket, point
`RECURSION_SRC_PATH` at a local `recursion_zkr.zip`. That is the official
upstream offline escape hatch: the build script verifies the artifact SHA-256
against a pinned digest instead of patching RISC Zero registry sources.
'@
$readme | Set-Content -Encoding ASCII (Join-Path $out "README.md")

$shaFile = Join-Path $out "SHA256SUMS"
$manifestFile = Join-Path $out "manifest.json"
$gitCommit = try { (git -C $repoRoot rev-parse HEAD 2>$null).Trim() } catch { "unknown" }
$gitDirty = $true
try {
    $gitDirty = -not [string]::IsNullOrWhiteSpace((git -C $repoRoot status --porcelain 2>$null))
} catch {
    $gitDirty = $true
}

$packageArtifacts | ForEach-Object {
    "{0} *{1}" -f $_.sha256, $_.name
} | Set-Content -Encoding ASCII -Path $shaFile

$manifest = [ordered]@{
    package = "hivemind-windows-worker"
    configuration = $Configuration
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    git_commit = $gitCommit
    git_dirty = $gitDirty
    artifacts = $packageArtifacts
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Encoding ASCII -Path $manifestFile

Write-Host "Windows worker package written to $out"
