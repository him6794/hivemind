param(
    [ValidateSet("release", "debug")][string]$Configuration = "release",
    [string]$OutputDir = "dist\windows-master-webview"
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$rustRoot = Join-Path $repoRoot "hivemind-rs"
$masterUiRoot = Join-Path $repoRoot "frontend\master-ui"
$masterUiDist = Join-Path $masterUiRoot "dist"
$out = Join-Path $repoRoot $OutputDir
$target = "x86_64-pc-windows-msvc"

if (Test-Path -LiteralPath $out) {
    $existing = @(Get-ChildItem -LiteralPath $out -Force)
    if ($existing.Count -gt 0) {
        throw "Output directory must be empty to avoid replacing an existing Master installation: $out"
    }
}

$artifactDir = Join-Path $repoRoot "vendor\libtailscale\windows-x86_64-msvc"
$libtailscale = Join-Path $artifactDir "libtailscale.dll"
if (!(Test-Path -LiteralPath $libtailscale -PathType Leaf)) {
    throw "Missing Windows x64 libtailscale.dll: $libtailscale"
}

$redistRoot = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\2022\BuildTools\VC\Redist\MSVC"
if (!(Test-Path -LiteralPath $redistRoot)) {
    throw "Visual C++ redistributable directory is required: $redistRoot"
}
$vcRuntime = Get-ChildItem -Path $redistRoot -Recurse -File -Filter "vcruntime140.dll" |
    Where-Object { $_.FullName -match "\\x64\\Microsoft\.VC[0-9]+\.CRT\\vcruntime140\.dll$" } |
    Sort-Object FullName -Descending |
    Select-Object -First 1
if ($null -eq $vcRuntime) {
    throw "Matching x64 vcruntime140.dll was not found below $redistRoot"
}
$vcRuntime140_1 = Get-ChildItem -Path $vcRuntime.DirectoryName -File -Filter "vcruntime140_1.dll" |
    Select-Object -First 1
if ($null -eq $vcRuntime140_1) {
    throw "Matching x64 vcruntime140_1.dll was not found beside $($vcRuntime.FullName)"
}

$previousViteApiBase = $env:VITE_API_BASE
try {
    # Packaged Master UI calls the same-origin Master HTTP API.
    $env:VITE_API_BASE = ""
    Push-Location $masterUiRoot
    try {
        & npm.cmd ci
        if ($LASTEXITCODE -ne 0) {
            throw "npm ci failed while preparing the Master UI."
        }
        & npm.cmd run build
        if ($LASTEXITCODE -ne 0) {
            throw "npm run build failed while preparing the Master UI."
        }
    } finally {
        Pop-Location
    }
} finally {
    if ($null -eq $previousViteApiBase) {
        Remove-Item Env:VITE_API_BASE -ErrorAction SilentlyContinue
    } else {
        $env:VITE_API_BASE = $previousViteApiBase
    }
}

if (!(Test-Path -LiteralPath (Join-Path $masterUiDist "index.html") -PathType Leaf)) {
    throw "Master UI build did not produce $masterUiDist\index.html"
}

Push-Location $rustRoot
try {
    $cargoArgs = @(
        "build", "--locked", "--target", $target,
        "--no-default-features", "--features", "master,master-webview",
        "--bin", "hivemind-master", "--bin", "hivemind-master-ui"
    )
    if ($Configuration -eq "release") {
        $cargoArgs += "--release"
    }

    $vsDevCmd = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
    if (!(Test-Path -LiteralPath $vsDevCmd)) {
        throw "Visual Studio Build Tools VsDevCmd.bat is required: $vsDevCmd"
    }
    $profile = if ($Configuration -eq "release") { "release" } else { "debug" }
    $cargoCommand = "cargo $($cargoArgs -join ' ')"
    $cmdLine = "call `"$vsDevCmd`" -arch=x64 -host_arch=x64 && set GOTELEMETRY=off && $cargoCommand"
    & cmd.exe /d /s /c $cmdLine
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo failed for $target with exit code $LASTEXITCODE."
    }

    $masterBinary = Join-Path $rustRoot "target\$target\$profile\hivemind-master.exe"
    $webviewBinary = Join-Path $rustRoot "target\$target\$profile\hivemind-master-ui.exe"
} finally {
    Pop-Location
}

foreach ($binary in @($masterBinary, $webviewBinary)) {
    if (!(Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Built Master executable not found: $binary"
    }
}

New-Item -ItemType Directory -Force -Path $out | Out-Null
Copy-Item -LiteralPath $masterBinary -Destination (Join-Path $out "hivemind-master.exe")
Copy-Item -LiteralPath $webviewBinary -Destination (Join-Path $out "hivemind-master-ui.exe")
Copy-Item -LiteralPath $libtailscale -Destination (Join-Path $out "libtailscale.dll")
Copy-Item -LiteralPath $vcRuntime.FullName -Destination (Join-Path $out "vcruntime140.dll")
Copy-Item -LiteralPath $vcRuntime140_1.FullName -Destination (Join-Path $out "vcruntime140_1.dll")
$packagedUi = Join-Path $out "master-ui"
New-Item -ItemType Directory -Force -Path $packagedUi | Out-Null
Copy-Item -Path (Join-Path $masterUiDist "*") -Destination $packagedUi -Recurse -Force

$readme = @'
# Hivemind Windows Master WebView Test Bundle

This is a local x64 MSVC test bundle, not a signed production release. It contains the Master binary, its isolated WebView helper, and the Master UI.

1. Start `hivemind-master.exe` and sign in to the Master UI with your Hivemind account.
2. On the first authenticated login, Master automatically requests one-time VPN enrollment from the configured Website API, joins the overlay, and waits for Nodepool readiness before enabling task operations.

For ordinary public-network use, you do not need to set `JWT_SECRET` or manually pin a Nodepool IP. The package contains no server secret, reusable VPN key, or fixed Nodepool address. For a private deployment, configure the Website API origin with `MASTER_WEBSITE_API_BASE` or `WEBSITE_API_BASE`; it must expose `POST /api/login` and the protected `POST /api/vpn/config` route. This is the Website API address, not a manually configured Nodepool address.

If you use an optional `.env`, place it beside `hivemind-master.exe`; only that file is considered, regardless of the launch directory. Parent workspace `.env` files are ignored. Explicit process environment variables and `HIVEMIND_CONFIG` still work.

The Master serves the bundled UI from `master-ui`. The optional WebView2 Runtime is not installed by this package. When the runtime is present, the UI opens in an embedded WebView2 window; otherwise startup falls back to the system browser. The WebView only navigates to this Master API's loopback origin. Closing the WebView does not stop the Master; stop the Master process separately when it is safe to do so. Set `HIVEMIND_DISABLE_OPEN_UI=1` to disable automatic UI opening.
'@
$readmePath = Join-Path $out "README.md"
$readme | Set-Content -LiteralPath $readmePath -Encoding ASCII

$checksumLines = @()
Get-ChildItem -LiteralPath $out -File -Recurse | Sort-Object FullName | ForEach-Object {
    $relativePath = $_.FullName.Substring($out.Length).TrimStart('\', '/') -replace '\\', '/'
    $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumLines += "$hash *$relativePath"
}
$checksumLines | Set-Content -LiteralPath (Join-Path $out "SHA256SUMS") -Encoding ASCII
Write-Host "Windows Master WebView test bundle written to $out"
