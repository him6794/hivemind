param(
    [string]$Configuration = "release",
    [ValidateSet("x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu", "aarch64-pc-windows-msvc")]
    [string]$RustTarget = "x86_64-pc-windows-msvc",
    [string]$OutputDir = "dist\windows-worker",
    [string]$NodepoolGrpcAddr = "",
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
$workerUiRoot = Join-Path $repoRoot "frontend\worker-ui"
$workerUiDist = Join-Path $workerUiRoot "dist"
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
    $cargoArgs = @("build", "--locked", "--target", $RustTarget, "--bin", "hivemind-worker")
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
        $cargoCommand = "cargo build --locked --target $RustTarget --bin hivemind-worker"
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
    $binary = Join-Path $rustRoot "target\$RustTarget\$profile\hivemind-worker.exe"
} finally {
    Pop-Location
}

if (!(Test-Path $binary)) {
    throw "Built Worker binary not found: $binary"
}

Push-Location $workerUiRoot
$previousWorkerControlBase = $env:VITE_WORKER_CONTROL_BASE
try {
    $workerControlBase = $WorkerControlHttpAddr.Trim()
    if ($workerControlBase -notmatch '^[a-zA-Z][a-zA-Z0-9+.-]*://') {
        $workerControlBase = "http://$workerControlBase"
    }
    if ($workerControlBase -match '^http://0\.0\.0\.0(?=[:/])') {
        $workerControlBase = $workerControlBase -replace '^http://0\.0\.0\.0', 'http://127.0.0.1'
    }
    $env:VITE_WORKER_CONTROL_BASE = $workerControlBase.TrimEnd('/')
    & npm ci
    if ($LASTEXITCODE -ne 0) {
        throw "npm ci failed while preparing the Worker UI."
    }
    & npm run build
    if ($LASTEXITCODE -ne 0) {
        throw "npm run build failed while preparing the Worker UI."
    }
} finally {
    if ($null -eq $previousWorkerControlBase) {
        Remove-Item Env:VITE_WORKER_CONTROL_BASE -ErrorAction SilentlyContinue
    } else {
        $env:VITE_WORKER_CONTROL_BASE = $previousWorkerControlBase
    }
    Pop-Location
}

if (!(Test-Path -LiteralPath (Join-Path $workerUiDist "index.html"))) {
    throw "Worker UI build did not produce $workerUiDist\index.html"
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
$staleAllInOneBinary = Join-Path $out "hivemind-bin.exe"
if (Test-Path -LiteralPath $staleAllInOneBinary) {
    Remove-Item -Force -LiteralPath $staleAllInOneBinary
}
$packagedBinary = Join-Path $out "hivemind-worker.exe"
Copy-Item -Force $binary $packagedBinary
$packagedWorkerUi = Join-Path $out "worker-ui"
if (Test-Path -LiteralPath $packagedWorkerUi) {
    Remove-Item -Recurse -Force -LiteralPath $packagedWorkerUi
}
New-Item -ItemType Directory -Force -Path $packagedWorkerUi | Out-Null
Copy-Item -Path (Join-Path $workerUiDist "*") -Destination $packagedWorkerUi -Recurse -Force
$packageArtifacts = @(
    [ordered]@{
        name = "hivemind-worker.exe"
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedBinary).Hash.ToLowerInvariant()
        source = $binary
    }
)
Get-ChildItem -LiteralPath $packagedWorkerUi -File -Recurse | ForEach-Object {
    $relativePath = $_.FullName.Substring($packagedWorkerUi.Length).TrimStart('\', '/')
    $packageArtifacts += [ordered]@{
        name = "worker-ui/$($relativePath -replace '\\', '/')"
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        source = $_.FullName
    }
}
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
# NODEPOOL_GRPC_ADDR and NODEPOOL_GRPC_ENDPOINT are compatibility settings for
# private deployments. Public onboarding discovers the transport automatically
# after website login, so both may stay blank.
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
# WORKER_ADVERTISE_ADDR is optional. Leave it blank for a session-only worker:
# tasks are delivered and results returned through the outbound worker session,
# so no inbound address is required.
WORKER_NODEPOOL_TOKEN=
# No static Worker ID or reusable nodepool token is required; identity is
# server-assigned at enrollment. These remain only as legacy/private overrides.
# Leave blank to use the target machine's COMPUTERNAME.
WORKER_ID=
WORKER_LOCATION=windows

JWT_SECRET=
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
# NODEPOOL_GRPC_ENDPOINT/NODEPOOL_GRPC_ADDR are optional: public onboarding
# discovers the platform transport after website login.

& (Join-Path $PSScriptRoot "hivemind-worker.exe")
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

1. Double-click `hivemind-worker.exe`. The Worker page opens in your browser.
2. Sign in with your Hivemind account. The Worker connects to the network, registers this machine, and starts accepting jobs automatically.
3. Keep the Worker window open while you want this machine to receive jobs. No `.env` file, terminal command, port choice, or key setup is needed.

For a private deployment or unattended startup, `.env.worker.example` and `start-worker.ps1` are available as optional advanced settings. The normal sign-in flow does not store your password, server key, or reusable VPN key.

The Worker runs on a suitable local Windows host. Orange Pi is reserved for Nodepool, Website API, Headscale, PostgreSQL, and Redis; do not deploy this Worker package there.
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
