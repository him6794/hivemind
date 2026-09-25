$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "package-worker-windows.ps1"
$scriptText = Get-Content -LiteralPath $scriptPath -Raw

if ($scriptText -match "(?i)monty") {
    throw "Windows worker packaging must not retain the removed Monty runtime contract."
}

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
    -Needle "function Reset-CmdConsoleOpacity" `
    -Message "start-worker launcher must reset cmd.exe console opacity before starting services."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WindowAlpha" `
    -Message "start-worker launcher must remove persisted Console WindowAlpha values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'New-ItemProperty -LiteralPath $key -Name "WindowAlpha" -Value 255 -PropertyType DWord -Force' `
    -Message "start-worker launcher must explicitly persist fully opaque cmd.exe console alpha."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "WindowTransparency" `
    -Message "start-worker launcher must remove persisted Console WindowTransparency values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "function Reset-CurrentConsoleOpacity" `
    -Message "start-worker launcher must reset the already-created console window, not only persisted registry values."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "GetConsoleWindow" `
    -Message "start-worker launcher must locate the current console window handle."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "SetLayeredWindowAttributes(`$consoleWindow, 0, 255, 0x2)" `
    -Message "start-worker launcher must force the current console window alpha to fully opaque."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Get-ChildItem -LiteralPath $consoleRoot -Recurse' `
    -Message "start-worker launcher must inspect all Console subkeys, including path-encoded cmd.exe keys."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Reset-CmdConsoleOpacity' `
    -Message "start-worker launcher must call the opacity reset function."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "function Reset-WindowsTerminalCmdOpacity" `
    -Message "start-worker launcher must reset Windows Terminal cmd.exe profile opacity, not only legacy conhost settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "Microsoft.WindowsTerminal_8wekyb3d8bbwe" `
    -Message "start-worker launcher must inspect packaged Windows Terminal settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "useAcrylic" `
    -Message "start-worker launcher must disable Windows Terminal acrylic transparency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "useAcrylicInTabRow" `
    -Message "start-worker launcher must disable Windows Terminal tab row acrylic transparency."

Assert-Contains `
    -Haystack $scriptText `
    -Needle "opacity" `
    -Message "start-worker launcher must force Windows Terminal profile opacity to 100."

$importCall = $scriptText.IndexOf("Import-DotEnv -Path `$envFile")
$resetCall = if ($importCall -ge 0) { $scriptText.LastIndexOf("Reset-WindowsTerminalCmdOpacity", $importCall) } else { -1 }
if ($resetCall -lt 0 -or $importCall -lt 0) {
    throw "start-worker launcher must reset console opacity before .env import or validation can abort startup."
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'function Ensure-JwtSecret' `
    -Message "start-worker launcher must generate an internal JWT secret when it is not already available."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Generated a process-local JWT_SECRET; no user-provided secret is needed.' `
    -Message "start-worker launcher must keep its generated JWT secret process-local and require no user-provided signing secret."
if ($scriptText -match 'Ensure-JwtSecret -Path \$envFile|Set-Content -LiteralPath \$Path -Value \$contents') {
    throw "start-worker launcher must not persist its generated JWT secret into the user configuration file."
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Assert-RequiredEnv -Names @("WORKER_GRPC_ADDR", "WORKER_CONTROL_HTTP_ADDR")' `
    -Message "start-worker launcher must require worker listen/control addresses without requiring a pre-provisioned token."

# Zero-config onboarding: neither the launcher nor the template may require a
# Nodepool endpoint, a static Worker ID, or a reusable nodepool token.
if ($scriptText -match 'Required setting NODEPOOL_GRPC_ENDPOINT') {
    throw "start-worker launcher must not require NODEPOOL_GRPC_ENDPOINT/NODEPOOL_GRPC_ADDR; public onboarding discovers the transport after login."
}

foreach ($forbiddenRequirement in @(
        'Assert-RequiredEnv[^\r\n]*WORKER_ID',
        'Assert-RequiredEnv[^\r\n]*NODEPOOL',
        'Assert-RequiredEnv[^\r\n]*WORKER_ADVERTISE_ADDR'
    )) {
    if ($scriptText -match $forbiddenRequirement) {
        throw "worker launcher must not require a static identity/endpoint setting matched by '$forbiddenRequirement'."
    }
}

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'NODEPOOL_GRPC_ADDR and NODEPOOL_GRPC_ENDPOINT are' `
    -Message "worker package template must mark both Nodepool endpoint settings as optional compatibility settings."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Ensure-JwtSecret' `
    -Message "start-worker launcher must initialize its process-local JWT secret without requiring user configuration."

Assert-Contains `
    -Haystack $scriptText `
    -Needle 'TORRENT_TASK_ARTIFACT_BASE_URL=' `
    -Message "worker package template must expose the remote task artifact base URL setting."

# The normal double-click package is a dedicated Worker application. It must
# not depend on the all-service binary or a role argument.
Assert-Contains `
    -Haystack $scriptText `
    -Needle '"--bin", "hivemind-worker"' `
    -Message "Windows worker packaging must build the dedicated hivemind-worker binary."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'hivemind-worker.exe' `
    -Message "Windows worker packaging must ship hivemind-worker.exe."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Output directory must be empty to avoid replacing an existing Worker installation' `
    -Message "Windows worker packaging must refuse to overwrite an existing output package."
foreach ($requiredWebviewContract in @(
        '$packageWebview = $RustTarget -eq "x86_64-pc-windows-msvc"',
        '"--bin", "hivemind-worker-ui"',
        '"--no-default-features", "--features"',
        '"worker,worker-webview"',
        'Copy-Item -Force $webviewBinary $packagedWebview',
        'name = "hivemind-worker-ui.exe"'
    )) {
    Assert-Contains -Haystack $scriptText -Needle $requiredWebviewContract `
        -Message "Windows worker packaging is missing WebView contract '$requiredWebviewContract'."
}
if ($scriptText -match '& \(Join-Path \$PSScriptRoot "hivemind-bin\.exe"\)') {
    throw "Windows worker launcher must not start the all-service hivemind-bin.exe."
}
if ($scriptText -match '& \(Join-Path \$PSScriptRoot "hivemind-worker\.exe"\) worker') {
    throw "Dedicated Windows worker launcher must not require a worker role argument."
}

# The package owns the browser surface: build it with the configured local
# control address and place it beside the executable for static serving.
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$workerUiRoot = Join-Path $repoRoot "frontend\worker-ui"' `
    -Message "Windows worker packaging must locate the Worker UI source."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '& npm ci' `
    -Message "Windows worker packaging must install the locked Worker UI dependencies."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '& npm run build' `
    -Message "Windows worker packaging must build the Worker UI."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$env:VITE_WORKER_CONTROL_BASE = $workerControlBase' `
    -Message "Windows worker packaging must bake the configured Worker Control address into the UI."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$workerControlBase = $workerControlBase -replace ''^http://\[::\]'', ''http://[::1]''' `
    -Message "Windows worker packaging must use IPv6 loopback for a wildcard listener."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$packagedWorkerUi = Join-Path $out "worker-ui"' `
    -Message "Windows worker packaging must create a beside-executable worker-ui directory."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Copy-Item -Path (Join-Path $workerUiDist "*") -Destination $packagedWorkerUi -Recurse -Force' `
    -Message "Windows worker packaging must copy the built Worker UI into the package."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'Test-Path -LiteralPath (Join-Path $workerUiDist "index.html")' `
    -Message "Windows worker packaging must verify the direct package UI entrypoint exists."
Assert-Contains `
    -Haystack $scriptText `
    -Needle ".TrimStart('\', '/')" `
    -Message "Windows worker packaging must normalize UI paths with a single Windows path separator."

# Native HCS assets are package-owned and loaded automatically. The public
# template must not turn an operator registry path into a user setting.
foreach ($requiredHcsBundleContract in @(
        '[string]$WindowsHcsRuntimeBundlePath = ""',
        '$packagedHcsRuntime = Join-Path $out "windows-hcs-runtime"',
        'WindowsHcsRuntimeBundlePath must point to a regular, non-reparse directory.',
        'Windows HCS runtime bundle must contain bundle-manifest.json.',
        'Windows HCS runtime bundle manifest must be a regular, non-reparse file.',
        'Windows HCS runtime bundle manifest must contain manifest and signature fields.',
        'Windows HCS runtime bundle cannot contain reparse points:',
        'Get-ChildItem -LiteralPath $bundleSource -Force',
        'Copy-Item -LiteralPath $bundleChild.FullName -Destination $packagedHcsRuntime -Recurse -Force',
        'name = "windows-hcs-runtime/'
    )) {
    Assert-Contains -Haystack $scriptText -Needle $requiredHcsBundleContract `
        -Message "Windows worker packaging is missing HCS bundle contract '$requiredHcsBundleContract'."
}

if ($scriptText -notmatch '\[string\]\$NodepoolGrpcAddr\s*=\s*""') {
    throw "Windows worker packaging must not use a fake Nodepool hostname as its default."
}
if ($scriptText -notmatch '\$packageArtifacts\s*\+=\s*\[ordered\]@\{\s*\r?\n\s*name\s*=\s*"worker-ui/') {
    throw "Windows worker package manifest must include hashes for the bundled Worker UI files."
}

# The package README is Markdown, and it must be built from a literal
# here-string: an interpolating one silently eats every backtick as an escape
# character, so the code spans and fenced blocks reach the package intact.
# Extracting it here also lets the assertions below run against the rendered
# text, which is what an operator actually reads, rather than the script source.
$readmeMatch = [regex]::Match($scriptText, "(?s)\`$readme = @'\r?\n(.*?)\r?\n'@")
if (!$readmeMatch.Success) {
    throw "windows worker package README must come from a literal here-string, or PowerShell strips its Markdown backticks."
}
$packagedReadme = $readmeMatch.Groups[1].Value

Assert-Contains `
    -Haystack $packagedReadme `
    -Needle '`.env.worker.example`' `
    -Message "packaged README must keep its Markdown inline code spans intact."
Assert-Contains `
    -Haystack $packagedReadme `
    -Needle 'WebView2 Runtime' `
    -Message "packaged README must explain the embedded window prerequisite."
Assert-Contains `
    -Haystack $packagedReadme `
    -Needle 'On the first authenticated login, the Worker automatically obtains one-time VPN enrollment' `
    -Message "packaged README must explain that first-login VPN enrollment is automatic."
Assert-Contains `
    -Haystack $packagedReadme `
    -Needle 'No `.env` file, terminal command, port choice, `JWT_SECRET`, manually fixed Nodepool IP' `
    -Message "packaged README must not require JWT_SECRET or a manually fixed Nodepool address for ordinary use."
Assert-Contains `
    -Haystack $packagedReadme `
    -Needle 'Closing only the WebView window does not stop the Worker' `
    -Message "packaged README must explain Worker lifetime after closing the window."

# The README is written with -Encoding ASCII, which would turn anything else
# into a literal '?' in the shipped package.
if ([regex]::IsMatch($packagedReadme, '[^\x00-\x7F]')) {
    throw "packaged README must stay ASCII-only because it is written with -Encoding ASCII."
}

# The generated env template must remain complete and must not contain server secrets.
$envMatch = [regex]::Match($scriptText, '(?s)\$envTemplate = @"\r?\n(.*?)\r?\n"@')
if (!$envMatch.Success) {
    throw "worker package must build .env.worker.example from a here-string."
}
$packagedEnv = $envMatch.Groups[1].Value
if ([regex]::IsMatch($packagedEnv, '(?m)^\s*HEADSCALE_API_KEY\s*=')) {
    throw "worker package must never distribute the server-side HEADSCALE_API_KEY."
}
if ([regex]::IsMatch($packagedEnv, '(?m)^\s*JWT_SECRET\s*=')) {
    throw "worker package template must not make a service JWT_SECRET look like an ordinary node setting."
}
if (![regex]::IsMatch($packagedEnv, '(?m)^WORKER_VPN_AUTHKEY=\r?$')) {
    throw "worker package template must not embed a VPN auth key."
}
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'VPN auth keys must be supplied at runtime; never embed them in a Worker package.' `
    -Message "worker packaging must reject explicit VPN auth keys before building."
foreach ($expected in @(
        "NODEPOOL_GRPC_ENDPOINT", "WEBSITE_API_BASE", "HEADSCALE_LOGIN_SERVER",
        "WORKER_VPN_AUTHKEY", "WORKER_VPN_HOSTNAME", "VPN_STARTUP_TIMEOUT_SECS",
        "UPDATE_ENABLED", "UPDATE_PRODUCT", "UPDATE_CHANNEL", "UPDATE_KEYSET_URL",
        "UPDATE_MANIFEST_URL", "UPDATE_ALLOWED_HOSTS", "UPDATE_INSTALL_ROOT",
        "UPDATE_STAGING_ROOT", "UPDATE_CHECK_INTERVAL_SECS", "UPDATE_MAX_PACKAGE_BYTES",
        "TORRENT_TASK_ARTIFACT_BASE_URL"
    )) {
    Assert-Contains -Haystack $packagedEnv -Needle $expected `
        -Message "worker package template must expose '$expected'."
}
if ($packagedEnv -match '(?m)^\s*HIVEMIND_GENERAL_COMPUTE_WINDOWS_BACKENDS\s*=') {
    throw "public worker package template must not require an HCS backend registry setting."
}
Assert-Contains `
    -Haystack $packagedEnv `
    -Needle 'Native Windows HCS general-compute support is loaded from the signed bundle' `
    -Message "worker package template must describe automatic signed HCS bundle loading."
foreach ($forbiddenRequirement in @(
        'Assert-RequiredEnv[^\r\n]*WORKER_VPN_AUTHKEY',
        'Assert-RequiredEnv[^\r\n]*WORKER_NODEPOOL_TOKEN',
        'Assert-RequiredEnv[^\r\n]*WORKER_ID',
        'Assert-RequiredEnv[^\r\n]*NODEPOOL'
    )) {
    if ($scriptText -match $forbiddenRequirement) {
        throw "worker launcher must not require a static setting matched by '$forbiddenRequirement'."
    }
}

# Release metadata is only an external signing input. The package must never
# emit a file whose name implies that unsigned bytes are update authority.
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$manifestFile = Join-Path $out "manifest.unsigned.json"' `
    -Message "Windows worker packaging must keep the provenance manifest explicitly unsigned."
Assert-Contains `
    -Haystack $scriptText `
    -Needle '$updateManifestFile = Join-Path $out "update-manifest.unsigned.json"' `
    -Message "Windows worker packaging must emit a distinct unsigned update-manifest signing input."
Assert-Contains `
    -Haystack $scriptText `
    -Needle 'external release signing is still required' `
    -Message "Windows worker packaging must require external release signing."
if ($scriptText -match '\$manifestFile\s*=\s*Join-Path \$out "manifest\.json"') {
    throw "Windows worker packaging must not emit an unsigned manifest.json update authority."
}
if ($scriptText -match '(?i)(private[_ -]?key|signing[_ -]?key)\s*=') {
    throw "Windows worker packaging must not accept or embed a release signing private key."
}
if ($scriptText -match 'WORKER_EXECUTION_PUBLIC_KEY_PEM') {
    throw "Windows release metadata must not reuse the Worker execution trust key."
}

foreach ($requiredUpdateContract in @(
        '[string]$PackageVersion = "0.1.0"',
        '[UInt64]$PackageSequence = 0',
        '[string]$MinimumSupportedVersion = ""',
        '[string]$ReleaseKeyId = ""',
        '[string]$UpdatePackageUrl = ""',
        '[string]$UpdatePackagePath = ""',
        'PackageSequence must be positive when update metadata is requested.',
        'PackageVersion must use canonical major.minor.patch form.',
        'MinimumSupportedVersion must use canonical major.minor.patch form.',
        'UpdatePackageUrl must be an HTTPS URL for update metadata.',
        'UpdatePackagePath must point to the verified package archive.',
        'UpdatePackagePath must not be a reparse point.',
        'canonical unsigned input for an external release signer'
    )) {
    Assert-Contains -Haystack $scriptText -Needle $requiredUpdateContract `
        -Message "Windows worker packaging is missing update contract '$requiredUpdateContract'."
}

Write-Host "package-worker-windows launcher and release-input tests passed"
