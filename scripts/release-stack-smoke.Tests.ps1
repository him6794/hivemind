$ErrorActionPreference = "Stop"

$scriptPath = Join-Path $PSScriptRoot "release-stack-smoke.ps1"
if (!(Test-Path -LiteralPath $scriptPath)) {
    throw "release-stack-smoke.ps1 must exist."
}

$scriptText = Get-Content -LiteralPath $scriptPath -Raw

foreach ($expected in @(
    "docker compose up -d --build",
    "docker compose down -v",
    "Wait-ForHttpOk",
    "SITE_HOST_PORT",
    "VITE_API_BASE",
    "VITE_WORKER_CONTROL_BASE",
    "POSTGRES_VOLUME_NAME",
    "REDIS_VOLUME_NAME",
    "HIVEMIND_DATA_VOLUME_NAME",
    "MASTER_CORS_ALLOWED_ORIGINS",
    "WORKER_CONTROL_CORS_ALLOWED_ORIGINS",
    "MASTER_UI_HOST_PORT",
    "WORKER_UI_HOST_PORT",
    "MASTER_HTTP_HOST_PORT",
    "WORKER_CONTROL_HOST_PORT"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must include '$expected'."
    }
}

if (!$scriptText.Contains('Match = "OK"')) {
    throw "release stack smoke harness must validate the master API health payload using the current uppercase 'OK' response."
}

foreach ($expected in @(
    "/api/worker-info",
    '"success":true'
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must validate the worker control API via '$expected'."
    }
}

foreach ($expected in @(
    "Test-ManagedProverLaunch",
    "docker compose exec -T worker",
    "managed proof generation failed"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must verify the packaged managed prover can launch via '$expected'."
    }
}

if (!$scriptText.Contains("docker-compose-release.Tests.ps1")) {
    throw "release stack smoke harness must validate compose packaging prerequisites before startup."
}

if (!$scriptText.Contains("-CheckOnly")) {
    throw "release stack smoke harness must support a check-only mode."
}

foreach ($expected in @(
    "POSTGRES_PASSWORD",
    "POSTGRES_HOST_PORT",
    "REDIS_HOST_PORT",
    "NODEPOOL_GRPC_HOST_PORT",
    "TORRENT_TRACKER_HOST_PORT",
    "TORRENT_SEED_HOST_PORT",
    "MASTER_HTTP_HOST_PORT",
    "WORKER_GRPC_HOST_PORT",
    "WORKER_CONTROL_HOST_PORT",
    "MASTER_UI_HOST_PORT",
    "WORKER_UI_HOST_PORT",
    "SITE_HOST_PORT",
    "VITE_API_BASE",
    "VITE_WORKER_CONTROL_BASE",
    "POSTGRES_VOLUME_NAME",
    "REDIS_VOLUME_NAME",
    "HIVEMIND_DATA_VOLUME_NAME",
    "JWT_SECRET",
    "WORKER_EXECUTION_PRIVATE_KEY_PEM",
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "change-me-in-production",
    "RandomNumberGenerator",
    "TcpListener",
    "Get-Command",
    "openssl",
    "OpenSSL is required",
    "Install OpenSSL",
    "genpkey",
    "Ed25519",
    "-pubout",
    "Remove-Item",
    'Remove-Item -LiteralPath "Env:$name"',
    "SetEnvironmentVariable"
)) {
    if (!$scriptText.Contains($expected)) {
        throw "release stack smoke harness must securely manage ephemeral release configuration via '$expected'."
    }
}

$managedEnvironmentNames = @(
    "POSTGRES_PASSWORD",
    "POSTGRES_HOST_PORT",
    "REDIS_HOST_PORT",
    "NODEPOOL_GRPC_HOST_PORT",
    "TORRENT_TRACKER_HOST_PORT",
    "TORRENT_SEED_HOST_PORT",
    "MASTER_HTTP_HOST_PORT",
    "WORKER_GRPC_HOST_PORT",
    "WORKER_CONTROL_HOST_PORT",
    "MASTER_UI_HOST_PORT",
    "WORKER_UI_HOST_PORT",
    "SITE_HOST_PORT",
    "REDIS_VOLUME_NAME",
    "POSTGRES_VOLUME_NAME",
    "HIVEMIND_DATA_VOLUME_NAME",
    "NODEPOOL_TORRENTS_VOLUME_NAME",
    "NODEPOOL_TASK_PACKAGES_VOLUME_NAME",
    "MASTER_TASK_REFERENCES_VOLUME_NAME",
    "MASTER_TORRENTS_VOLUME_NAME",
    "WORKER_TASK_DOWNLOADS_VOLUME_NAME",
    "WORKER_TORRENTS_VOLUME_NAME",
    "MASTER_CORS_ALLOWED_ORIGINS",
    "WORKER_CONTROL_CORS_ALLOWED_ORIGINS",
    "JWT_SECRET",
    "WORKER_EXECUTION_PRIVATE_KEY_PEM",
    "WORKER_EXECUTION_PUBLIC_KEY_PEM",
    "WORKER_NODEPOOL_TOKEN"
)
$originalEnvironment = @{}
foreach ($name in $managedEnvironmentNames) {
    $originalEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
}
$originalPath = $env:Path
$testOpenSslDirectory = Join-Path ([IO.Path]::GetTempPath()) ("hivemind-openssl-test-" + [guid]::NewGuid().ToString("N"))
[void](New-Item -ItemType Directory -Path $testOpenSslDirectory)
$testOpenSslPath = Join-Path $testOpenSslDirectory "openssl.cmd"
$testOpenSslScript = @'
@echo off
setlocal
set "operation=%~1"
set "output="
:parse
if "%~1"=="" goto write
if "%~1"=="-out" (
    set "output=%~2"
    shift
)
shift
goto parse
:write
if not defined output exit /b 2
if "%operation%"=="genpkey" (
    >"%output%" echo -----BEGIN PRIVATE KEY-----
    >>"%output%" echo deterministic-release-smoke-private-key
    >>"%output%" echo -----END PRIVATE KEY-----
    exit /b 0
)
if "%operation%"=="pkey" (
    >"%output%" echo -----BEGIN PUBLIC KEY-----
    >>"%output%" echo deterministic-release-smoke-public-key
    >>"%output%" echo -----END PUBLIC KEY-----
    exit /b 0
)
exit /b 3
'@
[IO.File]::WriteAllText($testOpenSslPath, $testOpenSslScript, (New-Object Text.ASCIIEncoding))
$env:Path = "${testOpenSslDirectory};${originalPath}"

function Get-ReleaseStackSmokeTempDirectories {
    return @(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter "hivemind-release-stack-smoke-*" -ErrorAction SilentlyContinue |
        ForEach-Object { $_.FullName })
}

try {
    foreach ($name in $managedEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }

    $tempDirectoriesBefore = @(Get-ReleaseStackSmokeTempDirectories)
    $missingEnvironmentOutput = $null
    try {
        $missingEnvironmentOutput = @(& $scriptPath -CheckOnly *>&1)
    }
    catch {
        $joinedOutput = (@($missingEnvironmentOutput) + $_ | ForEach-Object { $_.ToString() }) -join "`n"
        throw "release-stack-smoke.ps1 -CheckOnly must succeed without pre-set release environment variables.`n${joinedOutput}"
    }

    foreach ($name in $managedEnvironmentNames) {
        if ($null -ne [Environment]::GetEnvironmentVariable($name, "Process")) {
            throw "release-stack-smoke.ps1 must restore absent ${name} after check-only completes."
        }
    }

    $newTempDirectories = @(Get-ReleaseStackSmokeTempDirectories | Where-Object { $tempDirectoriesBefore -notcontains $_ })
    if ($newTempDirectories.Count -ne 0) {
        throw "release-stack-smoke.ps1 must remove ephemeral key directories after check-only: $($newTempDirectories -join ', ')"
    }

    $missingEnvironmentText = ($missingEnvironmentOutput | ForEach-Object { $_.ToString() }) -join "`n"
    if ([string]::IsNullOrWhiteSpace($missingEnvironmentText)) {
        $joinedOutput = $missingEnvironmentOutput -join "`n"
        throw "release-stack-smoke.ps1 -CheckOnly must report its ephemeral setup and result.`n${joinedOutput}"
    }

    foreach ($expectedOutput in @(
        "SET POSTGRES_PASSWORD",
        "SET POSTGRES_HOST_PORT",
        "SET REDIS_HOST_PORT",
        "SET NODEPOOL_GRPC_HOST_PORT",
        "SET TORRENT_TRACKER_HOST_PORT",
        "SET TORRENT_SEED_HOST_PORT",
        "SET MASTER_HTTP_HOST_PORT",
        "SET WORKER_GRPC_HOST_PORT",
        "SET WORKER_CONTROL_HOST_PORT",
        "SET MASTER_UI_HOST_PORT",
        "SET WORKER_UI_HOST_PORT",
        "SET SITE_HOST_PORT",
        "SET VITE_API_BASE",
        "SET VITE_WORKER_CONTROL_BASE",
        "SET POSTGRES_VOLUME_NAME",
        "SET REDIS_VOLUME_NAME",
        "SET HIVEMIND_DATA_VOLUME_NAME",
        "SET NODEPOOL_TORRENTS_VOLUME_NAME",
        "SET NODEPOOL_TASK_PACKAGES_VOLUME_NAME",
        "SET MASTER_TASK_REFERENCES_VOLUME_NAME",
        "SET MASTER_TORRENTS_VOLUME_NAME",
        "SET WORKER_TASK_DOWNLOADS_VOLUME_NAME",
        "SET WORKER_TORRENTS_VOLUME_NAME",
        "SET MASTER_CORS_ALLOWED_ORIGINS",
        "SET WORKER_CONTROL_CORS_ALLOWED_ORIGINS",
        "SET JWT_SECRET",
        "SET WORKER_EXECUTION_PRIVATE_KEY_PEM",
        "SET WORKER_EXECUTION_PUBLIC_KEY_PEM",
        "release stack smoke check-only passed"
    )) {
        if (!$missingEnvironmentText.Contains($expectedOutput)) {
            throw "Missing-environment check-only run must report '$expectedOutput'."
        }
    }

    $suppliedEnvironment = @{
        POSTGRES_PASSWORD = "user-supplied-postgres-password"
        POSTGRES_HOST_PORT = "15432"
        REDIS_HOST_PORT = "16379"
        NODEPOOL_GRPC_HOST_PORT = "15051"
        TORRENT_TRACKER_HOST_PORT = "16969"
        TORRENT_SEED_HOST_PORT = "16881"
        MASTER_HTTP_HOST_PORT = "18082"
        WORKER_GRPC_HOST_PORT = "15053"
        WORKER_CONTROL_HOST_PORT = "18080"
        MASTER_UI_HOST_PORT = "13000"
        WORKER_UI_HOST_PORT = "13001"
        SITE_HOST_PORT = "18000"
        VITE_API_BASE = "http://127.0.0.1:18082"
        VITE_WORKER_CONTROL_BASE = "http://127.0.0.1:18080"
        POSTGRES_VOLUME_NAME = "hivemind-test-postgres"
        REDIS_VOLUME_NAME = "hivemind-test-redis"
        HIVEMIND_DATA_VOLUME_NAME = "hivemind-test-data"
        NODEPOOL_TORRENTS_VOLUME_NAME = "hivemind-test-nodepool-torrents"
        NODEPOOL_TASK_PACKAGES_VOLUME_NAME = "hivemind-test-nodepool-packages"
        MASTER_TASK_REFERENCES_VOLUME_NAME = "hivemind-test-master-references"
        MASTER_TORRENTS_VOLUME_NAME = "hivemind-test-master-torrents"
        WORKER_TASK_DOWNLOADS_VOLUME_NAME = "hivemind-test-worker-downloads"
        WORKER_TORRENTS_VOLUME_NAME = "hivemind-test-worker-torrents"
        MASTER_CORS_ALLOWED_ORIGINS = "http://127.0.0.1:13000,http://127.0.0.1:13001"
        WORKER_CONTROL_CORS_ALLOWED_ORIGINS = "http://127.0.0.1:13001"
        JWT_SECRET = "user-supplied-jwt-secret-with-at-least-32-bytes"
        WORKER_EXECUTION_PRIVATE_KEY_PEM = "user-supplied-private-key"
        WORKER_EXECUTION_PUBLIC_KEY_PEM = "user-supplied-public-key"
    }
    foreach ($name in $managedEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }
    foreach ($name in $suppliedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $suppliedEnvironment[$name], "Process")
    }

    $suppliedEnvironmentOutput = $null
    try {
        $suppliedEnvironmentOutput = @(& $scriptPath -CheckOnly *>&1)
    }
    catch {
        $joinedOutput = (@($suppliedEnvironmentOutput) + $_ | ForEach-Object { $_.ToString() }) -join "`n"
        throw "release-stack-smoke.ps1 -CheckOnly must accept user-supplied release environment variables.`n${joinedOutput}"
    }

    $suppliedJoinedOutput = ($suppliedEnvironmentOutput | ForEach-Object { $_.ToString() }) -join "`n"
    foreach ($name in $suppliedEnvironment.Keys) {
        if ([Environment]::GetEnvironmentVariable($name, "Process") -cne $suppliedEnvironment[$name]) {
            throw "release stack smoke harness must preserve user-supplied ${name} exactly."
        }
        if ($suppliedJoinedOutput.Contains("SET ${name}")) {
            throw "release stack smoke harness must not replace user-supplied ${name}."
        }
    }

    foreach ($name in $managedEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }
    [Environment]::SetEnvironmentVariable("POSTGRES_PASSWORD", "private-only-postgres-password", "Process")
    [Environment]::SetEnvironmentVariable("JWT_SECRET", "private-only-jwt-secret-with-at-least-32-bytes", "Process")
    [Environment]::SetEnvironmentVariable("WORKER_EXECUTION_PRIVATE_KEY_PEM", "private-only-test-key", "Process")
    $tempDirectoriesBefore = @(Get-ReleaseStackSmokeTempDirectories)
    $privateOnlyOutput = @(& $scriptPath -CheckOnly *>&1)
    $privateOnlyText = ($privateOnlyOutput | ForEach-Object { $_.ToString() }) -join "`n"

    if (!$privateOnlyText.Contains("SET WORKER_EXECUTION_PUBLIC_KEY_PEM") -or
        $privateOnlyText.Contains("SET WORKER_EXECUTION_PRIVATE_KEY_PEM")) {
        throw "A supplied private key must derive only the missing public key."
    }
    if ([Environment]::GetEnvironmentVariable("WORKER_EXECUTION_PRIVATE_KEY_PEM", "Process") -cne "private-only-test-key" -or
        $null -ne [Environment]::GetEnvironmentVariable("WORKER_EXECUTION_PUBLIC_KEY_PEM", "Process")) {
        throw "Private-only check-only must preserve the supplied private key and restore the absent public key."
    }
    $newTempDirectories = @(Get-ReleaseStackSmokeTempDirectories | Where-Object { $tempDirectoriesBefore -notcontains $_ })
    if ($newTempDirectories.Count -ne 0) {
        throw "Private-only check-only must remove its temporary key directory: $($newTempDirectories -join ', ')"
    }

    foreach ($name in $managedEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }
    [Environment]::SetEnvironmentVariable("POSTGRES_PASSWORD", "public-only-postgres-password", "Process")
    [Environment]::SetEnvironmentVariable("JWT_SECRET", "public-only-jwt-secret-with-at-least-32-bytes", "Process")
    [Environment]::SetEnvironmentVariable("WORKER_EXECUTION_PUBLIC_KEY_PEM", "public-only-test-key", "Process")
    $publicOnlyError = $null
    try {
        & $scriptPath -CheckOnly *> $null
    }
    catch {
        $publicOnlyError = $_.Exception.Message
    }
    if ([string]::IsNullOrWhiteSpace($publicOnlyError) -or
        !$publicOnlyError.Contains("WORKER_EXECUTION_PRIVATE_KEY_PEM is missing")) {
        throw "A public key without its private key must fail with an actionable pairing error. Error: $publicOnlyError"
    }
    if ($null -ne [Environment]::GetEnvironmentVariable("WORKER_EXECUTION_PRIVATE_KEY_PEM", "Process") -or
        [Environment]::GetEnvironmentVariable("WORKER_EXECUTION_PUBLIC_KEY_PEM", "Process") -cne "public-only-test-key") {
        throw "Public-only failure must restore the absent private key and preserve the supplied public key."
    }

    foreach ($name in $managedEnvironmentNames) {
        Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
    }
    $env:Path = ""
    try {
        $powerShellExecutable = if ($PSEdition -eq "Core") {
            Join-Path $PSHOME "pwsh.exe"
        }
        else {
            Join-Path $PSHOME "powershell.exe"
        }
        if (!(Test-Path -LiteralPath $powerShellExecutable -PathType Leaf)) {
            throw "Unable to locate the current PowerShell executable at $powerShellExecutable."
        }

        $previousErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $missingOpenSslOutput = @(& $powerShellExecutable -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $scriptPath -CheckOnly *>&1)
            $missingOpenSslExitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $previousErrorActionPreference
        }
        $missingOpenSslText = ($missingOpenSslOutput | ForEach-Object { $_.ToString() }) -join "`n"
        if ($missingOpenSslExitCode -eq 0 -or
            !$missingOpenSslText.Contains("OpenSSL is required") -or
            !$missingOpenSslText.Contains("Install OpenSSL")) {
            throw "release stack smoke harness must fail actionably when OpenSSL is unavailable. exit=$missingOpenSslExitCode Error: $missingOpenSslText"
        }
    }
    finally {
        $env:Path = $originalPath
    }
}
finally {
    $env:Path = $originalPath
    foreach ($name in $managedEnvironmentNames) {
        $originalValue = $originalEnvironment[$name]
        if ($null -eq $originalValue) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        }
        else {
            [Environment]::SetEnvironmentVariable($name, $originalValue, "Process")
        }
    }
    if (Test-Path -LiteralPath $testOpenSslDirectory) {
        Remove-Item -LiteralPath $testOpenSslDirectory -Recurse -Force
    }
}

Write-Host "release stack smoke tests passed"
