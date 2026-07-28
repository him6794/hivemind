$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "release-stack-smoke.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "release-stack-smoke.ps1 must exist."
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw

foreach ($expected in @(
    "docker compose up -d --build",
    "docker compose down",
    "Wait-ForHttpOk",
    "http://127.0.0.1:8080",
    "http://127.0.0.1:3000",
    "http://127.0.0.1:3001",
    "http://127.0.0.1:8082/health",
    "http://127.0.0.1:18080"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must include '$expected'."
    }
}

if (!$scriptText.Contains('Match = "OK"')) {
    throw "release stack smoke harness must validate the master API health payload using the current uppercase 'OK' response."
}

foreach ($expected in @(
    "http://127.0.0.1:18080/api/worker-info",
    '"success":true'
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must validate the worker control API via '$expected'."
    }
}

if (!$scriptText.Contains("docker-compose-release.Tests.ps1")) {
    throw "release stack smoke harness must validate compose packaging prerequisites before startup."
}

if (!$scriptText.Contains("-CheckOnly")) {
    throw "release stack smoke harness must support a check-only mode."
}

foreach ($expected in @(
    "JWT_SECRET",
    "change-me-in-production",
    "SetEnvironmentVariable"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must override default JWT_SECRET handling via '$expected'."
    }
}

Write-Host "release stack smoke tests passed"
