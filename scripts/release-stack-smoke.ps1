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
$managedEnvironmentNames = @(
    "POSTGRES_PASSWORD",
    "POSTGRES_HOST_PORT",
    "REDIS_HOST_PORT",
    "JWT_SECRET",
    "WORKER_EXECUTION_PRIVATE_KEY_PEM",
    "WORKER_EXECUTION_PUBLIC_KEY_PEM"
)
$originalEnvironment = @{}
foreach ($name in $managedEnvironmentNames) {
    $originalEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}
$restoreEnvironmentNames = @()
$temporaryKeyDirectory = $null
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

function New-SecureEphemeralSecret {
    param([string]$Prefix)

    $randomBytes = New-Object byte[] 32
    $randomNumberGenerator = [Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $randomNumberGenerator.GetBytes($randomBytes)
    }
    finally {
        $randomNumberGenerator.Dispose()
    }

    $randomHex = ([BitConverter]::ToString($randomBytes)).Replace("-", "").ToLowerInvariant()
    return "${Prefix}${randomHex}"
}

function Get-AvailableHostPort {
    param([int[]]$ExcludedPorts = @())

    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        $listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
        try {
            $listener.Start()
            $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
        }
        finally {
            $listener.Stop()
        }

        if ($ExcludedPorts -notcontains $port) {
            return $port
        }
    }

    throw "Unable to reserve distinct host ports for the release smoke infrastructure."
}

function Get-OpenSslCommand {
    $openSsl = Get-Command "openssl" -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $openSsl) {
        throw "OpenSSL is required to generate the ephemeral Ed25519 worker execution key pair. Install OpenSSL or supply matching WORKER_EXECUTION_PRIVATE_KEY_PEM and WORKER_EXECUTION_PUBLIC_KEY_PEM values."
    }

    return $openSsl.Source
}

try {
    $reservedHostPorts = @()
    foreach ($name in @("REDIS_HOST_PORT", "POSTGRES_HOST_PORT")) {
        $configuredPort = $originalEnvironment[$name]
        if (![string]::IsNullOrWhiteSpace($configuredPort)) {
            $parsedPort = 0
            if ([int]::TryParse($configuredPort, [ref]$parsedPort)) {
                $reservedHostPorts += $parsedPort
            }
        }
    }

    foreach ($name in @("REDIS_HOST_PORT", "POSTGRES_HOST_PORT")) {
        if ([string]::IsNullOrWhiteSpace($originalEnvironment[$name])) {
            $ephemeralHostPort = Get-AvailableHostPort -ExcludedPorts $reservedHostPorts
            [Environment]::SetEnvironmentVariable($name, [string]$ephemeralHostPort, "Process")
            $reservedHostPorts += $ephemeralHostPort
            $restoreEnvironmentNames += $name
            Write-Host "SET ${name} to collision-free ephemeral host port ${ephemeralHostPort}"
        }
    }

    $originalPostgresPassword = $originalEnvironment["POSTGRES_PASSWORD"]
    if ([string]::IsNullOrWhiteSpace($originalPostgresPassword)) {
        $ephemeralPostgresPassword = New-SecureEphemeralSecret -Prefix "release-stack-smoke-postgres-"
        [Environment]::SetEnvironmentVariable("POSTGRES_PASSWORD", $ephemeralPostgresPassword, "Process")
        $restoreEnvironmentNames += "POSTGRES_PASSWORD"
        Write-Host "SET POSTGRES_PASSWORD to a secure ephemeral release smoke password"
    }

    $originalJwtSecret = $originalEnvironment["JWT_SECRET"]
    if ([string]::IsNullOrWhiteSpace($originalJwtSecret) -or $originalJwtSecret -eq "change-me-in-production") {
        $ephemeralJwtSecret = New-SecureEphemeralSecret -Prefix "release-stack-smoke-jwt-"
        [Environment]::SetEnvironmentVariable("JWT_SECRET", $ephemeralJwtSecret, "Process")
        $restoreEnvironmentNames += "JWT_SECRET"
        Write-Host "SET JWT_SECRET to an ephemeral non-default release smoke secret"
    }

    $originalPrivateKey = $originalEnvironment["WORKER_EXECUTION_PRIVATE_KEY_PEM"]
    $originalPublicKey = $originalEnvironment["WORKER_EXECUTION_PUBLIC_KEY_PEM"]
    $privateKeyMissing = [string]::IsNullOrWhiteSpace($originalPrivateKey)
    $publicKeyMissing = [string]::IsNullOrWhiteSpace($originalPublicKey)

    if ($privateKeyMissing -and !$publicKeyMissing) {
        throw "WORKER_EXECUTION_PRIVATE_KEY_PEM is missing while WORKER_EXECUTION_PUBLIC_KEY_PEM is set. Supply the matching private key or unset both values so the smoke harness can generate an ephemeral Ed25519 pair."
    }

    if ($privateKeyMissing -or $publicKeyMissing) {
        $openSslCommand = Get-OpenSslCommand
        $temporaryKeyDirectory = Join-Path ([IO.Path]::GetTempPath()) ("hivemind-release-stack-smoke-" + [guid]::NewGuid().ToString("N"))
        [void](New-Item -ItemType Directory -Path $temporaryKeyDirectory)
        $privateKeyPath = Join-Path $temporaryKeyDirectory "worker-execution-private.pem"
        $publicKeyPath = Join-Path $temporaryKeyDirectory "worker-execution-public.pem"

        if ($privateKeyMissing) {
            Invoke-CheckedCommand -Command $openSslCommand -Arguments @(
                "genpkey",
                "-algorithm",
                "Ed25519",
                "-out",
                $privateKeyPath
            ) -WorkingDirectory $temporaryKeyDirectory

            $ephemeralPrivateKey = [IO.File]::ReadAllText($privateKeyPath)
            [Environment]::SetEnvironmentVariable("WORKER_EXECUTION_PRIVATE_KEY_PEM", $ephemeralPrivateKey, "Process")
            $restoreEnvironmentNames += "WORKER_EXECUTION_PRIVATE_KEY_PEM"
            Write-Host "SET WORKER_EXECUTION_PRIVATE_KEY_PEM to an ephemeral Ed25519 release smoke key"
        }
        else {
            $utf8WithoutBom = New-Object Text.UTF8Encoding($false)
            [IO.File]::WriteAllText($privateKeyPath, $originalPrivateKey, $utf8WithoutBom)
        }

        Invoke-CheckedCommand -Command $openSslCommand -Arguments @(
            "pkey",
            "-in",
            $privateKeyPath,
            "-pubout",
            "-out",
            $publicKeyPath
        ) -WorkingDirectory $temporaryKeyDirectory

        $ephemeralPublicKey = [IO.File]::ReadAllText($publicKeyPath)
        [Environment]::SetEnvironmentVariable("WORKER_EXECUTION_PUBLIC_KEY_PEM", $ephemeralPublicKey, "Process")
        $restoreEnvironmentNames += "WORKER_EXECUTION_PUBLIC_KEY_PEM"
        Write-Host "SET WORKER_EXECUTION_PUBLIC_KEY_PEM to the matching ephemeral Ed25519 release smoke key"
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
    }
    else {
        Write-Host "RUN docker compose up -d --build"
        Invoke-CheckedCommand -Command "docker" -Arguments @("compose", "up", "-d", "--build") -WorkingDirectory $repoRoot

        foreach ($service in $services) {
            Write-Host "WAIT $($service.Name) $($service.Uri)"
            Wait-ForHttpOk -Uri $service.Uri -ExpectedContent $service.Match -TimeoutSeconds $StartupTimeoutSeconds
            Write-Host "PASS $($service.Name) $($service.Uri)"
        }

        Write-Host "release stack smoke passed for official site, customer app, worker app, api, and worker control"
    }
}
finally {
    if (!$CheckOnly -and !$KeepRunning) {
        try {
            Write-Host "RUN docker compose down"
            Invoke-CheckedCommand -Command "docker" -Arguments @("compose", "down") -WorkingDirectory $repoRoot
        }
        catch {
            Write-Warning "docker compose down failed during cleanup: $($_.Exception.Message)"
        }
    }

    foreach ($name in $restoreEnvironmentNames) {
        [Environment]::SetEnvironmentVariable($name, $originalEnvironment[$name], "Process")
    }

    if ($null -ne $temporaryKeyDirectory -and (Test-Path -LiteralPath $temporaryKeyDirectory)) {
        Remove-Item -LiteralPath $temporaryKeyDirectory -Recurse -Force
    }
}
