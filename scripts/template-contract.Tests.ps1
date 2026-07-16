$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$pythonTemplates = @("python-script", "data-processing", "batch-render")

foreach ($templateName in $pythonTemplates) {
    $templateDir = Join-Path $repoRoot "templates\$templateName"
    $entrypoint = Join-Path $templateDir "main.py"
    $manifest = Get-Content -LiteralPath (Join-Path $templateDir "task.json") -Raw | ConvertFrom-Json

    if (!(Test-Path -LiteralPath $entrypoint)) {
        throw "Template $templateName must provide top-level main.py for the Rust worker ZIP contract"
    }

    if ($manifest.entrypoint -ne "main.py") {
        throw "Template $templateName must declare main.py as its entrypoint"
    }
}

$readme = Get-Content -LiteralPath (Join-Path $repoRoot "templates\README.md") -Raw
if (!$readme.Contains('top-level `main.py`')) {
    throw "Template README must document the Rust worker ZIP entrypoint contract"
}

Write-Host "template contract tests passed"
