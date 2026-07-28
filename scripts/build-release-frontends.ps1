param(
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$surfaces = @(
    @{
        Name = "official-site"
        Path = "frontend"
        Artifact = ".next/standalone/server.js"
        Clean = @(".next")
    },
    @{
        Name = "master-ui"
        Path = "frontend/master-ui"
        Artifact = "dist/index.html"
        Clean = @("dist")
    },
    @{
        Name = "worker-ui"
        Path = "frontend/worker-ui"
        Artifact = "dist/index.html"
        Clean = @("dist")
    }
)

function Invoke-CheckedCommand {
    param(
        [string]$Command,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )

    Write-Host "RUN $Command $($Arguments -join ' ') [$WorkingDirectory]"
    $previousLocation = Get-Location
    try {
        Set-Location -LiteralPath $WorkingDirectory
        & $Command @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Command exited with code $LASTEXITCODE in $WorkingDirectory."
        }
    }
    finally {
        Set-Location $previousLocation
    }
}

foreach ($surface in $surfaces) {
    $surfacePath = Join-Path $repoRoot $surface.Path
    $packagePath = Join-Path $surfacePath "package.json"
    if (!(Test-Path -LiteralPath $packagePath)) {
        throw "Missing package.json for $($surface.Name) at $surfacePath."
    }

    if (!$CheckOnly) {
        foreach ($relativeCleanPath in $surface.Clean) {
            $cleanPath = Join-Path $surfacePath $relativeCleanPath
            if (Test-Path -LiteralPath $cleanPath) {
                Remove-Item -LiteralPath $cleanPath -Recurse -Force
                Write-Host "CLEAN $($surface.Name) removed stale $cleanPath"
            }
        }

        Invoke-CheckedCommand -Command "npm.cmd" -Arguments @("run", "build") -WorkingDirectory $surfacePath
    }

    $artifactPath = Join-Path $surfacePath $surface.Artifact
    if (!(Test-Path -LiteralPath $artifactPath)) {
        $hint = if ($CheckOnly) { "Run scripts/build-release-frontends.ps1 first." } else { "npm run build did not produce $($surface.Artifact)." }
        throw "Missing build artifact for $($surface.Name): $artifactPath. $hint"
    }

    Write-Host "CHECK $($surface.Name) artifact $artifactPath"
}

Write-Host "release frontend build passed for official-site, master-ui, worker-ui"
