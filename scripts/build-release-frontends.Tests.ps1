$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "build-release-frontends.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "build-release-frontends.ps1 must exist."
}

$output = & powershell -NoProfile -ExecutionPolicy Bypass -File $scriptPath -CheckOnly 2>&1
$exitCode = $LASTEXITCODE
$joinedOutput = ($output | Out-String)

if ($exitCode -ne 0) {
    throw "build-release-frontends.ps1 -CheckOnly failed with exit code ${exitCode}.`n${joinedOutput}"
}

foreach ($expected in @("official-site", "master-ui", "worker-ui")) {
    if (!$joinedOutput.Contains($expected)) {
        throw "build release harness output must include surface '$expected' but did not."
    }
    if (!$joinedOutput.Contains("CHECK $expected artifact")) {
        throw "build release harness must emit CHECK line for '$expected' but did not."
    }
}

if (!$joinedOutput.Contains(".next\standalone\server.js")) {
    throw "build release harness must check the official Next standalone server artifact."
}

if (!$joinedOutput.Contains("master-ui\dist\index.html") -or !$joinedOutput.Contains("worker-ui\dist\index.html")) {
    throw "build release harness must continue checking Vite dist/index.html artifacts for master-ui and worker-ui."
}

Write-Host "build release frontend tests passed"
