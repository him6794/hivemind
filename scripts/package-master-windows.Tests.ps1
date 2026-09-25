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
        '"master,master-webview"',
        '"--bin", "hivemind-master", "--bin", "hivemind-master-ui"',
        '$env:VITE_API_BASE = ""',
        'Copy-Item -LiteralPath $masterBinary -Destination (Join-Path $out "hivemind-master.exe")',
        'Copy-Item -LiteralPath $webviewBinary -Destination (Join-Path $out "hivemind-master-ui.exe")',
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

if ($scriptText -match '(?i)(JWT_SECRET|NODEPOOL.*TOKEN|VPN_AUTHKEY)\s*=\s*[^"\r\n]+') {
    throw "Master test bundle script must not provision or embed deployment secrets."
}

Write-Host "package-master-windows release bundle tests passed"
