$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "package-master-windows.ps1"
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

foreach ($requiredContract in @(
        '$target = "x86_64-pc-windows-msvc"',
        '"--no-default-features", "--features", "master,master-webview"',
        '"--bin", "hivemind-master", "--bin", "hivemind-master-ui"',
        'call `"$vsDevCmd`" -arch=x64 -host_arch=x64',
        '$env:VITE_API_BASE = ""',
        'Copy-Item -LiteralPath $masterBinary -Destination (Join-Path $out "hivemind-master.exe")',
        'Copy-Item -LiteralPath $webviewBinary -Destination (Join-Path $out "hivemind-master-ui.exe")',
        'Copy-Item -LiteralPath $libtailscale -Destination (Join-Path $out "libtailscale.dll")',
        'Copy-Item -LiteralPath $vcRuntime.FullName -Destination (Join-Path $out "vcruntime140.dll")',
        'Copy-Item -LiteralPath $vcRuntime140_1.FullName -Destination (Join-Path $out "vcruntime140_1.dll")',
        '$packagedUi = Join-Path $out "master-ui"',
        'Copy-Item -Path (Join-Path $masterUiDist "*") -Destination $packagedUi -Recurse -Force',
        'HIVEMIND_DISABLE_OPEN_UI=1',
        'Output directory must be empty to avoid replacing an existing Master installation',
        'On the first authenticated login, Master automatically requests one-time VPN enrollment',
        'you do not need to set `JWT_SECRET` or manually pin a Nodepool IP',
        'MASTER_WEBSITE_API_BASE',
        'Parent workspace `.env` files are ignored.',
        'SHA256SUMS'
    )) {
    Assert-Contains -Haystack $scriptText -Needle $requiredContract `
        -Message "Master Windows packaging is missing contract '$requiredContract'."
}

$cargoToml = Get-Content -LiteralPath (Join-Path $PSScriptRoot "..\\hivemind-rs\\crates\\hivemind-bin\\Cargo.toml") -Raw
foreach ($roleIsolationContract in @(
        'master-webview = ["master", "local-ui"]',
        'required-features = ["master-webview"]'
    )) {
    Assert-Contains -Haystack $cargoToml -Needle $roleIsolationContract `
        -Message "Master UI helper role isolation is missing '$roleIsolationContract'."
}

$tauriRoot = Join-Path $PSScriptRoot "..\\hivemind-rs\\crates\\hivemind-bin"
$capability = Get-Content -LiteralPath (Join-Path $tauriRoot "capabilities\\local-ui.json") -Raw | ConvertFrom-Json
if ($capability.identifier -ne "local-ui" -or
    @($capability.windows).Count -ne 1 -or $capability.windows[0] -ne "local-ui" -or
    @($capability.platforms).Count -ne 1 -or $capability.platforms[0] -ne "windows" -or
    @($capability.permissions).Count -ne 0 -or
    ($capability.PSObject.Properties.Name -contains "remote")) {
    throw "Local UI Tauri capability must be Windows-only, match only local-ui, and grant no permissions or remote-domain access."
}
$tauriConfig = Get-Content -LiteralPath (Join-Path $tauriRoot "tauri.conf.json") -Raw | ConvertFrom-Json
if (@($tauriConfig.app.windows).Count -ne 0 -or
    $tauriConfig.app.withGlobalTauri -ne $false -or
    @($tauriConfig.app.security.capabilities).Count -ne 1 -or
    $tauriConfig.app.security.capabilities[0] -ne "local-ui" -or
    $tauriConfig.bundle.active -ne $false) {
    throw "Local UI Tauri config must create its window dynamically, expose no global API, use only its empty capability, and package no bundled assets."
}

$webviewSource = Get-Content -LiteralPath (Join-Path $tauriRoot "src\\local_ui_webview.rs") -Raw
foreach ($nativeRestriction in @(
        'policy_for_navigation.allows_navigation_to(destination)',
        'BOOTSTRAP_URL: &str = "about:blank"',
        'NewWindowResponse::Deny',
        'DownloadEvent::Requested',
        '.devtools(false)',
        '.incognito(true)',
        'SetAreDefaultContextMenusEnabled(false)',
        'SetAreDevToolsEnabled(false)',
        'SetAreBrowserAcceleratorKeysEnabled(false)',
        'HIVEMIND_LOCAL_UI_READY\n',
        'watch_parent_stdin(app_handle)'
    )) {
    Assert-Contains -Haystack $webviewSource -Needle $nativeRestriction `
        -Message "Local UI helper is missing required native restriction '$nativeRestriction'."
}
if ($webviewSource -match '\.plugin\s*\(|\.invoke_handler\s*\(') {
    throw "Local UI helper must not register Tauri plugins or command handlers."
}
$masterUiSource = Get-Content -LiteralPath (Join-Path $tauriRoot "src\\bin\\hivemind-master-ui.rs") -Raw
foreach ($helperTargetContract in @(
        '#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]',
        '#[cfg(not(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc")))]',
        'hivemind-master-ui requires x86_64-pc-windows-msvc'
    )) {
    Assert-Contains -Haystack $masterUiSource -Needle $helperTargetContract `
        -Message "Master helper must restrict Tauri imports to x64 MSVC and provide a safe fallback ('$helperTargetContract')."
}
foreach ($forbiddenWebviewRuntimeAsset in @("WebView2Loader.dll", "msedgewebview2.exe", "FixedVersionRuntime")) {
    if ($scriptText.Contains($forbiddenWebviewRuntimeAsset)) {
        throw "Master package must not include WebView2 runtime asset '$forbiddenWebviewRuntimeAsset'."
    }
}

$readmeMatch = [regex]::Match($scriptText, "(?s)\`$readme = @'\r?\n(.*?)\r?\n'@")
if (!$readmeMatch.Success) {
    throw "Master package README must come from a literal here-string."
}
$packagedReadme = $readmeMatch.Groups[1].Value
foreach ($webviewDocumentationContract in @(
        'optional WebView2 Runtime is not installed by this package',
        'falls back to the system browser'
    )) {
    Assert-Contains -Haystack $packagedReadme -Needle $webviewDocumentationContract `
        -Message "Master README must explain the optional, unbundled WebView2 fallback contract '$webviewDocumentationContract'."
}

if ($scriptText -match '(?i)(JWT_SECRET|NODEPOOL.*TOKEN|VPN_AUTHKEY)\s*=\s*[^"\r\n]+') {
    throw "Master test bundle script must not provision or embed deployment secrets."
}

Write-Host "package-master-windows release bundle tests passed"
