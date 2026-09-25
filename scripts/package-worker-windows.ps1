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
    [string]$WorkerControlHttpAddr = "127.0.0.1:18080",
    [ValidateSet("stable", "beta", "nightly")][string]$UpdateChannel = "stable",
    [string]$PackageVersion = "0.1.0",
    [UInt64]$PackageSequence = 0,
    [string]$MinimumSupportedVersion = "",
    [UInt64]$IssuedAtUnix = 0,
    [UInt64]$ExpiresAtUnix = 0,
    [string]$ReleaseKeyId = "",
    [string]$UpdatePackageUrl = "",
    [string]$UpdatePackagePath = "",
    [string]$WindowsHcsRuntimeBundlePath = ""
)

$ErrorActionPreference = "Stop"

if (-not [string]::IsNullOrWhiteSpace($WorkerVpnAuthkey)) {
    throw "VPN auth keys must be supplied at runtime; never embed them in a Worker package."
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$rustRoot = Join-Path $repoRoot "hivemind-rs"
$workerUiRoot = Join-Path $repoRoot "frontend\worker-ui"
$workerUiDist = Join-Path $workerUiRoot "dist"
$out = Join-Path $repoRoot $OutputDir

if (Test-Path -LiteralPath $out) {
    $existing = @(Get-ChildItem -LiteralPath $out -Force)
    if ($existing.Count -gt 0) {
        throw "Output directory must be empty to avoid replacing an existing Worker installation: $out"
    }
}

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

$packageWebview = $RustTarget -eq "x86_64-pc-windows-msvc"
Push-Location $rustRoot
try {
    $cargoArgs = @("build", "--locked", "--target", $RustTarget, "--bin", "hivemind-worker")
    if ($packageWebview) {
        $cargoArgs += @("--bin", "hivemind-worker-ui")
    }
    $cargoArgs += @("--no-default-features", "--features", $(if ($packageWebview) { "worker,worker-webview" } else { "worker" }))
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
        $cargoCommand = "cargo $($cargoArgs -join ' ')"
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
    if ($packageWebview) {
        $webviewBinary = Join-Path $rustRoot "target\$RustTarget\$profile\hivemind-worker-ui.exe"
    }
} finally {
    Pop-Location
}

if (!(Test-Path $binary)) {
    throw "Built Worker binary not found: $binary"
}
if ($packageWebview -and !(Test-Path -LiteralPath $webviewBinary -PathType Leaf)) {
    throw "Built Worker WebView helper not found: $webviewBinary"
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
    if ($workerControlBase -match '^http://\[::\](?=[:/])') {
        $workerControlBase = $workerControlBase -replace '^http://\[::\]', 'http://[::1]'
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
$packagedWebview = Join-Path $out "hivemind-worker-ui.exe"
if ($packageWebview) {
    Copy-Item -Force $webviewBinary $packagedWebview
} elseif (Test-Path -LiteralPath $packagedWebview) {
    Remove-Item -Force -LiteralPath $packagedWebview
}
$packagedWorkerUi = Join-Path $out "worker-ui"
if (Test-Path -LiteralPath $packagedWorkerUi) {
    Remove-Item -Recurse -Force -LiteralPath $packagedWorkerUi
}
New-Item -ItemType Directory -Force -Path $packagedWorkerUi | Out-Null
Copy-Item -Path (Join-Path $workerUiDist "*") -Destination $packagedWorkerUi -Recurse -Force

$packagedHcsRuntime = Join-Path $out "windows-hcs-runtime"
if (Test-Path -LiteralPath $packagedHcsRuntime) {
    Remove-Item -Recurse -Force -LiteralPath $packagedHcsRuntime
}
if (-not [string]::IsNullOrWhiteSpace($WindowsHcsRuntimeBundlePath)) {
    if (!(Test-Path -LiteralPath $WindowsHcsRuntimeBundlePath -PathType Container)) {
        throw "WindowsHcsRuntimeBundlePath must point to a bundle directory."
    }
    $bundleSource = (Resolve-Path -LiteralPath $WindowsHcsRuntimeBundlePath).Path
    $bundleSourceItem = Get-Item -LiteralPath $bundleSource -Force
    if (!($bundleSourceItem -is [IO.DirectoryInfo]) -or
        (($bundleSourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "WindowsHcsRuntimeBundlePath must point to a regular, non-reparse directory."
    }
    $bundleManifestSource = Join-Path $bundleSource "bundle-manifest.json"
    if (!(Test-Path -LiteralPath $bundleManifestSource -PathType Leaf)) {
        throw "Windows HCS runtime bundle must contain bundle-manifest.json."
    }
    $bundleManifestItem = Get-Item -LiteralPath $bundleManifestSource -Force
    if (!($bundleManifestItem -is [IO.FileInfo]) -or
        (($bundleManifestItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "Windows HCS runtime bundle manifest must be a regular, non-reparse file."
    }
    try {
        $signedBundleManifest = Get-Content -LiteralPath $bundleManifestSource -Raw | ConvertFrom-Json
    } catch {
        throw "Windows HCS runtime bundle manifest must be valid JSON: $($_.Exception.Message)"
    }
    if ($null -eq $signedBundleManifest.manifest -or
        [string]::IsNullOrWhiteSpace([string]$signedBundleManifest.signature)) {
        throw "Windows HCS runtime bundle manifest must contain manifest and signature fields."
    }
    foreach ($bundleEntry in @(Get-ChildItem -LiteralPath $bundleSource -Force -Recurse)) {
        if (($bundleEntry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Windows HCS runtime bundle cannot contain reparse points: $($bundleEntry.FullName)"
        }
        if (!$bundleEntry.PSIsContainer -and !($bundleEntry -is [IO.FileInfo])) {
            throw "Windows HCS runtime bundle contains an unsupported filesystem entry: $($bundleEntry.FullName)"
        }
    }
    New-Item -ItemType Directory -Force -Path $packagedHcsRuntime | Out-Null
    foreach ($bundleChild in @(Get-ChildItem -LiteralPath $bundleSource -Force)) {
        Copy-Item -LiteralPath $bundleChild.FullName -Destination $packagedHcsRuntime -Recurse -Force
    }
    foreach ($bundleEntry in @(Get-ChildItem -LiteralPath $packagedHcsRuntime -Force -Recurse)) {
        if (($bundleEntry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Packaged Windows HCS runtime bundle contains a reparse point: $($bundleEntry.FullName)"
        }
    }
}

$packageArtifacts = @(
    [ordered]@{
        name = "hivemind-worker.exe"
        size = [UInt64](Get-Item -LiteralPath $packagedBinary).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedBinary).Hash.ToLowerInvariant()
        source = $binary
    }
)
if ($packageWebview) {
    $packageArtifacts += [ordered]@{
        name = "hivemind-worker-ui.exe"
        size = [UInt64](Get-Item -LiteralPath $packagedWebview).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedWebview).Hash.ToLowerInvariant()
        source = $webviewBinary
    }
}
Get-ChildItem -LiteralPath $packagedWorkerUi -File -Recurse | ForEach-Object {
    $relativePath = $_.FullName.Substring($packagedWorkerUi.Length).TrimStart('\', '/')
    $packageArtifacts += [ordered]@{
        name = "worker-ui/$($relativePath -replace '\\', '/')"
        size = [UInt64]$_.Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        source = $_.FullName
    }
}
if (Test-Path -LiteralPath $packagedHcsRuntime -PathType Container) {
    Get-ChildItem -LiteralPath $packagedHcsRuntime -File -Recurse | ForEach-Object {
        $relativePath = $_.FullName.Substring($packagedHcsRuntime.Length).TrimStart('\', '/')
        $packageArtifacts += [ordered]@{
            name = "windows-hcs-runtime/$($relativePath -replace '\\', '/')"
            size = [UInt64]$_.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
            source = $_.FullName
        }
    }
}
if ($RustTarget -like "*-pc-windows-msvc") {
    $packagedLibtailscale = Join-Path $out "libtailscale.dll"
    Copy-Item -Force $archive $packagedLibtailscale
    $packageArtifacts += [ordered]@{
        name = "libtailscale.dll"
        size = [UInt64](Get-Item -LiteralPath $packagedLibtailscale).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedLibtailscale).Hash.ToLowerInvariant()
        source = $archive
    }
    $packagedVcRuntime = Join-Path $out "vcruntime140.dll"
    Copy-Item -Force $vcRuntimeSource.FullName $packagedVcRuntime
    $packageArtifacts += [ordered]@{
        name = "vcruntime140.dll"
        size = [UInt64](Get-Item -LiteralPath $packagedVcRuntime).Length
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
WORKER_VPN_AUTHKEY=
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

# Signed updates stay deferred until an authenticated release endpoint and
# approved host are configured. No unsigned package is ever used as a fallback.
UPDATE_ENABLED=true
UPDATE_PRODUCT=
UPDATE_CHANNEL=$UpdateChannel
UPDATE_KEYSET_URL=
UPDATE_MANIFEST_URL=
UPDATE_ALLOWED_HOSTS=
UPDATE_INSTALL_ROOT=
UPDATE_STAGING_ROOT=
UPDATE_CHECK_INTERVAL_SECS=21600
UPDATE_MAX_PACKAGE_BYTES=8589934592

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
# Native Windows HCS general-compute support is loaded from the signed bundle
# beside this executable. Missing or invalid runtime assets keep it unavailable.
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
    $jwtSecret = [Environment]::GetEnvironmentVariable("JWT_SECRET", "Process")
    if (-not [string]::IsNullOrWhiteSpace($jwtSecret) -and
        -not $jwtSecret.Trim().Equals("CHANGE_ME_IN_PRODUCTION", [StringComparison]::OrdinalIgnoreCase) -and
        -not $jwtSecret.Trim().Equals("change-me-in-production", [StringComparison]::OrdinalIgnoreCase)) {
        return
    }

    $jwtSecret = New-RandomJwtSecret
    [Environment]::SetEnvironmentVariable("JWT_SECRET", $jwtSecret, "Process")
    Write-Host "Generated a process-local JWT_SECRET; no user-provided secret is needed."
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
Ensure-JwtSecret
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

1. Double-click `hivemind-worker.exe`. In the x64 MSVC package, the Worker page opens in an embedded WebView2 window when the WebView2 Runtime is available; otherwise it opens in your browser.
2. Sign in with your Hivemind account. On the first authenticated login, the Worker automatically obtains one-time VPN enrollment, joins the network, waits for Nodepool readiness, and registers this machine.
3. Keep the Worker process running while you want this machine to receive jobs. Closing only the WebView window does not stop the Worker; reopen `http://127.0.0.1:18080/` (or your configured control address) in your browser if needed. No `.env` file, terminal command, port choice, `JWT_SECRET`, manually fixed Nodepool IP, or reusable VPN key setup is needed.

For a private deployment or unattended startup, `.env.worker.example` and `start-worker.ps1` are available as optional advanced settings. The normal sign-in flow does not store your password, server key, or reusable VPN key.

`manifest.unsigned.json` and `update-manifest.unsigned.json` are build inputs only. They are not update authorities; release publication requires a root-verified keyset and an independently signed manifest.

When general-compute support is included, the package also contains a signed Windows HCS runtime bundle with its guest runner and image identity. It loads automatically after sign-in; no registry, image, runner, or execution settings are needed. If the bundle, policy, assets, or native HCS provider cannot be verified, general-compute stays unavailable instead of running on the host.

The Worker runs on a suitable local Windows host. Orange Pi is reserved for Nodepool, Website API, Headscale, PostgreSQL, and Redis; do not deploy this Worker package there.
'@
$readme | Set-Content -Encoding ASCII (Join-Path $out "README.md")

$shaFile = Join-Path $out "SHA256SUMS"
$manifestFile = Join-Path $out "manifest.unsigned.json"
$legacyManifestFile = Join-Path $out "manifest.json"
$updateManifestFile = Join-Path $out "update-manifest.unsigned.json"
foreach ($metadataPath in @($shaFile, $manifestFile, $legacyManifestFile, $updateManifestFile)) {
    if (Test-Path -LiteralPath $metadataPath) {
        Remove-Item -Force -LiteralPath $metadataPath
    }
}

$gitCommit = try { (git -C $repoRoot rev-parse HEAD 2>$null).Trim() } catch { "unknown" }
$gitDirty = $true
try {
    $gitDirty = -not [string]::IsNullOrWhiteSpace((git -C $repoRoot status --porcelain 2>$null))
} catch {
    $gitDirty = $true
}

# Only known package files enter the release inventory. This avoids silently
# shipping an old or operator-created file left in the output directory.
$packageFiles = @()
$packageFiles += [ordered]@{
    name = "hivemind-worker.exe"
    size = [UInt64](Get-Item -LiteralPath $packagedBinary).Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedBinary).Hash.ToLowerInvariant()
}
if ($packageWebview) {
    $packageFiles += [ordered]@{
        name = "hivemind-worker-ui.exe"
        size = [UInt64](Get-Item -LiteralPath $packagedWebview).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagedWebview).Hash.ToLowerInvariant()
    }
}
Get-ChildItem -LiteralPath $packagedWorkerUi -File -Recurse | ForEach-Object {
    $relativePath = $_.FullName.Substring($packagedWorkerUi.Length).TrimStart('\', '/')
    $packageFiles += [ordered]@{
        name = "worker-ui/$($relativePath -replace '\\', '/')"
        size = [UInt64]$_.Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    }
}
if (Test-Path -LiteralPath $packagedHcsRuntime -PathType Container) {
    Get-ChildItem -LiteralPath $packagedHcsRuntime -File -Recurse | ForEach-Object {
        $relativePath = $_.FullName.Substring($packagedHcsRuntime.Length).TrimStart('\', '/')
        $packageFiles += [ordered]@{
            name = "windows-hcs-runtime/$($relativePath -replace '\\', '/')"
            size = [UInt64]$_.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        }
    }
}
foreach ($optionalPackageFile in @(
        (Join-Path $out "libtailscale.dll"),
        (Join-Path $out "vcruntime140.dll"),
        (Join-Path $out "native-dependency-provenance.json"),
        (Join-Path $out ".env.worker.example"),
        (Join-Path $out "start-worker.ps1"),
        (Join-Path $out "README.md")
    )) {
    if (Test-Path -LiteralPath $optionalPackageFile) {
        $file = Get-Item -LiteralPath $optionalPackageFile
        $packageFiles += [ordered]@{
            name = $file.Name
            size = [UInt64]$file.Length
            sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant()
        }
    }
}
$packageFiles = @($packageFiles | Sort-Object -Property name)

$packageFiles | ForEach-Object {
    "{0} *{1}" -f $_.sha256, $_.name
} | Set-Content -Encoding ASCII -Path $shaFile
$shaMetadata = Get-Item -LiteralPath $shaFile
$packageFiles += [ordered]@{
    name = "SHA256SUMS"
    size = [UInt64]$shaMetadata.Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $shaFile).Hash.ToLowerInvariant()
}
$packageFiles = @($packageFiles | Sort-Object -Property name)

$manifest = [ordered]@{
    package = "hivemind-windows-worker"
    configuration = $Configuration
    generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
    git_commit = $gitCommit
    git_dirty = $gitDirty
    artifacts = $packageFiles
}
$manifest | ConvertTo-Json -Depth 10 | Set-Content -Encoding ASCII -Path $manifestFile

$updateMetadataRequested = $PackageSequence -ne 0 -or
    -not [string]::IsNullOrWhiteSpace($ReleaseKeyId) -or
    -not [string]::IsNullOrWhiteSpace($UpdatePackageUrl) -or
    -not [string]::IsNullOrWhiteSpace($UpdatePackagePath) -or
    -not [string]::IsNullOrWhiteSpace($MinimumSupportedVersion) -or
    $IssuedAtUnix -ne 0 -or
    $ExpiresAtUnix -ne 0
if ($updateMetadataRequested) {
    if ($PackageSequence -le 0) {
        throw "PackageSequence must be positive when update metadata is requested."
    }
    if ($PackageVersion -notmatch '^\d+\.\d+\.\d+$') {
        throw "PackageVersion must use canonical major.minor.patch form."
    }
    if ([string]::IsNullOrWhiteSpace($MinimumSupportedVersion)) {
        $MinimumSupportedVersion = $PackageVersion
    }
    if ($MinimumSupportedVersion -notmatch '^\d+\.\d+\.\d+$') {
        throw "MinimumSupportedVersion must use canonical major.minor.patch form."
    }
    if ([string]::IsNullOrWhiteSpace($ReleaseKeyId)) {
        throw "ReleaseKeyId is required for update metadata."
    }
    if ([string]::IsNullOrWhiteSpace($UpdatePackageUrl) -or $UpdatePackageUrl -notmatch '^https://') {
        throw "UpdatePackageUrl must be an HTTPS URL for update metadata."
    }
    if ([string]::IsNullOrWhiteSpace($UpdatePackagePath) -or !(Test-Path -LiteralPath $UpdatePackagePath -PathType Leaf)) {
        throw "UpdatePackagePath must point to the verified package archive."
    }
    $packagePathItem = Get-Item -LiteralPath $UpdatePackagePath
    if ($packagePathItem.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "UpdatePackagePath must not be a reparse point."
    }
    if ($packagePathItem.Length -le 0) {
        throw "UpdatePackagePath must not be empty."
    }
    if ($IssuedAtUnix -eq 0) {
        $IssuedAtUnix = [UInt64][DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    }
    if ($ExpiresAtUnix -eq 0) {
        $ExpiresAtUnix = $IssuedAtUnix + 30 * 24 * 60 * 60
    }
    if ($ExpiresAtUnix -le $IssuedAtUnix) {
        throw "ExpiresAtUnix must be later than IssuedAtUnix."
    }
    $architecture = if ($RustTarget.StartsWith("aarch64-")) { "aarch64" } else { "x86_64" }
    $unsignedUpdateManifest = [ordered]@{
        schema_version = 1
        product = "hivemind-windows-worker"
        channel = $UpdateChannel
        platform = "windows"
        architecture = $architecture
        version = $PackageVersion
        sequence = [UInt64]$PackageSequence
        minimum_supported_version = $MinimumSupportedVersion
        release_key_id = $ReleaseKeyId
        issued_at_unix = [UInt64]$IssuedAtUnix
        expires_at_unix = [UInt64]$ExpiresAtUnix
        package_url = $UpdatePackageUrl
        package_size = [UInt64]$packagePathItem.Length
        package_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $packagePathItem.FullName).Hash.ToLowerInvariant()
        files = @($packageFiles | ForEach-Object {
            [ordered]@{
                path = $_.name
                size = [UInt64]$_.size
                sha256 = $_.sha256
            }
        })
    }
    # This is canonical unsigned input for an external release signer. It is
    # never accepted as an update manifest and must not be published as one.
    $canonicalJson = $unsignedUpdateManifest | ConvertTo-Json -Depth 10 -Compress
    [IO.File]::WriteAllText($updateManifestFile, $canonicalJson, [Text.Encoding]::ASCII)
    Write-Host "Unsigned update manifest input written to $updateManifestFile; external release signing is still required."
}

Write-Host "Windows worker package written to $out"
