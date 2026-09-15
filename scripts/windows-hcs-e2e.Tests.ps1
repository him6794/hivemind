$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "windows-hcs-e2e.ps1"
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

Assert-Contains $scriptText 'Get-WindowsOptionalFeature -Online -FeatureName Containers' `
    "HCS E2E gate must verify the native Windows Containers feature."
Assert-Contains $scriptText 'Get-Service -Name vmcompute' `
    "HCS E2E gate must verify the HCS compute service."
Assert-Contains $scriptText 'provider = "hcs-windows-containers"' `
    "HCS E2E evidence must identify the native Windows provider."
Assert-Contains $scriptText 'exit 2' `
    "Missing HCS prerequisites must fail closed with a distinct blocked status."
Assert-Contains $scriptText 'windows-hcs-runtime' `
    "HCS E2E gate must require the package-relative HCS runtime bundle."
Assert-Contains $scriptText 'bundle-manifest.json' `
    "HCS E2E gate must require the signed package bundle manifest."
Assert-Contains $scriptText 'hcs_runtime_bundle' `
    "HCS E2E evidence must identify the packaged HCS runtime bundle."

if ($scriptText -match '(?i)docker|wsl|linux vm|direct process|powershell\.exe|cmd\.exe') {
    throw "Native Windows HCS E2E gate must not contain fallback execution paths."
}

Write-Host "windows-hcs-e2e prerequisite contract tests passed"
