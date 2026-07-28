param(
    [switch]$CheckOnly,
    [switch]$KeepRunning,
    [ValidateRange(1, 900)]
    [int]$StartupTimeoutSeconds = 180
)

# Supports -CheckOnly to validate release packaging prerequisites without starting containers.

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$packagingTestPath = Join-Path $PSScriptRoot "docker-compose-release.Tests.ps1"
$originalJwtSecret = [Environment]::GetEnvironmentVariable("JWT_SECRET", "Process")
$restoreJwtSecret = $false
$services = @(
    @{
        Name = "official-site"
        Uri = "http://127.0.0.1:8080"
        Match = "<html"
    },
    @{
        Name = "master-ui"
        Uri = "http://127.0.0.1:3000"
        Match = '<div id="root">'
    },
    @{
        Name = "worker-ui"
        Uri = "http://127.0.0.1:3001"
        Match = '<div id="root">'
    },
    @{
        Name = "master-api"
        Uri = "http://127.0.0.1:8082/health"
        Match = "OK"
    },
    @{
        Name = "worker-control"
        Uri = "http://127.0.0.1:18080/api/worker-info"
        Match = '"success":true'
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

function Wait-ForHttpOk {
    param(
        [string]$Uri,
        [string]$ExpectedContent,
        [int]$TimeoutSeconds
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $lastError = $null
    while ((Get-Date) -lt $deadline) {
        try {
            $response = Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 5
            if ($response.StatusCode -ge 200 -and $response.StatusCode -lt 400) {
                if ([string]::IsNullOrEmpty($ExpectedContent) -or $response.Content.Contains($ExpectedContent)) {
                    return
                }
                $lastError = "Unexpected content from $Uri status=$($response.StatusCode)"
            }
            else {
                $lastError = "Unexpected response from $Uri status=$($response.StatusCode)"
            }
        }
        catch {
            $lastError = $_.Exception.Message
        }

        Start-Sleep -Seconds 2
    }

    throw "Timed out waiting for $Uri. Last error: $lastError"
}

try {
    if ([string]::IsNullOrWhiteSpace($originalJwtSecret) -or $originalJwtSecret -eq "change-me-in-production") {
        $ephemeralJwtSecret = "release-stack-smoke-" + [guid]::NewGuid().ToString("N")
        [Environment]::SetEnvironmentVariable("JWT_SECRET", $ephemeralJwtSecret, "Process")
        $restoreJwtSecret = $true
        Write-Host "SET JWT_SECRET to an ephemeral non-default release smoke secret"
    }

    if (!(Test-Path -LiteralPath $packagingTestPath)) {
        throw "Missing compose packaging prerequisite test: $packagingTestPath"
    }

    Write-Host "PREREQ powershell -NoProfile -ExecutionPolicy Bypass -File scripts/docker-compose-release.Tests.ps1"
    Invoke-CheckedCommand -Command "powershell" -Arguments @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $packagingTestPath
    ) -WorkingDirectory $repoRoot

    if ($CheckOnly) {
        Write-Host "release stack smoke check-only passed"
        exit 0
    }

    Write-Host "RUN docker compose up -d --build"
    Invoke-CheckedCommand -Command "docker" -Arguments @("compose", "up", "-d", "--build") -WorkingDirectory $repoRoot

    foreach ($service in $services) {
        Write-Host "WAIT $($service.Name) $($service.Uri)"
        Wait-ForHttpOk -Uri $service.Uri -ExpectedContent $service.Match -TimeoutSeconds $StartupTimeoutSeconds
        Write-Host "PASS $($service.Name) $($service.Uri)"
    }

    Write-Host "release stack smoke passed for official site, customer app, worker app, api, and worker control"
}
finally {
    if ($restoreJwtSecret) {
        [Environment]::SetEnvironmentVariable("JWT_SECRET", $originalJwtSecret, "Process")
    }

    if (!$CheckOnly -and !$KeepRunning) {
        try {
            Write-Host "RUN docker compose down"
            Invoke-CheckedCommand -Command "docker" -Arguments @("compose", "down") -WorkingDirectory $repoRoot
        }
        catch {
            Write-Warning "docker compose down failed during cleanup: $($_.Exception.Message)"
        }
    }
}
