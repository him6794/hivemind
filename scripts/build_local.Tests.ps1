$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$scriptPath = Join-Path $PSScriptRoot "build_local.py"

if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "build_local.py must exist."
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw

foreach ($expected in @(
    'repo_root / "frontend",',
    'repo_root / "frontend" / "master-ui"',
    'repo_root / "frontend" / "worker-ui"'
)) {
    if (!$scriptText.Contains($expected)) {
        throw "build_local.py --frontend/--all must build surface $expected."
    }
}

if (!$scriptText.Contains("build-release-frontends.ps1") -and !$scriptText.Contains('repo_root / "frontend"')) {
    throw "build_local.py must include the official frontend build, not only master-ui and worker-ui."
}

Write-Host "build_local tests passed"
