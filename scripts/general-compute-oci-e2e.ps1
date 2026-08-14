[CmdletBinding()]
param(
    [switch]$CheckOnly,
    [switch]$Run,
    [string]$RegistryPath,
    [string]$ComposeFile,
    [string]$ProjectName = "hivemind-general-compute-oci-e2e"
)

$ErrorActionPreference = "Stop"

function Fail-Contract {
    param([Parameter(Mandatory = $true)][string]$Message)
    throw "general-compute OCI E2E preflight failed closed: $Message"
}

# Keep the harness vocabulary aligned with the runtime's fail-closed errors:
# an unpinned runner is `RunnerNotPinned`, and an OCI bundle with a default
# seccomp action of `SCMP_ACT_ERRNO` is not executable unless the reviewed
# operator profile is present. The preflight never translates either state
# into a direct host process.

function Require-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (!(Get-Command $Name -ErrorAction SilentlyContinue)) {
        Fail-Contract "required command '$Name' is not installed"
    }
}

function Require-AbsolutePath {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Value
    )
    if (![System.IO.Path]::IsPathRooted($Value)) {
        Fail-Contract "$Name must be an absolute operator-owned path"
    }
}

function Require-RegularFile {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if (!(Test-Path -LiteralPath $Path -PathType Leaf)) {
        Fail-Contract "$Name is missing or is not a regular file: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail-Contract "$Name must not be a symlink/reparse point: $Path"
    }
}

function Require-Directory {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if (!(Test-Path -LiteralPath $Path -PathType Container)) {
        Fail-Contract "$Name is missing or is not a directory: $Path"
    }
    $item = Get-Item -LiteralPath $Path -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Fail-Contract "$Name must not be a symlink/reparse point: $Path"
    }
}

function Require-Sha256 {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Value
    )
    if ($Value -notmatch '^sha256:[0-9a-fA-F]{64}$') {
        Fail-Contract "$Name must be a sha256:<64 hex> digest"
    }
}

function Require-SeccompProfile {
    param(
        [Parameter(Mandatory = $true)]$Registration,
        [Parameter(Mandatory = $true)][string]$BackendId
    )
    $profilePath = [string]$Registration.seccomp_profile_path
    Require-AbsolutePath "backend '$BackendId' seccomp profile" $profilePath
    Require-RegularFile "backend '$BackendId' seccomp profile" $profilePath

    $expectedDigest = [string]$Registration.policy.seccomp.profile_sha256
    Require-Sha256 "backend '$BackendId' seccomp profile" $expectedDigest
    $actualDigest = (Get-FileHash -LiteralPath $profilePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualDigest -ne $expectedDigest.Substring(7).ToLowerInvariant()) {
        Fail-Contract "backend '$BackendId' seccomp profile SHA-256 does not match the operator pin"
    }

    try {
        $profile = Get-Content -LiteralPath $profilePath -Raw | ConvertFrom-Json
    } catch {
        Fail-Contract "backend '$BackendId' seccomp profile is not valid JSON"
    }
    if ($null -eq $profile) {
        Fail-Contract "backend '$BackendId' seccomp profile must be a JSON object"
    }
    $profileFields = @($profile.PSObject.Properties.Name)
    if (($profileFields | Where-Object { $_ -notin @("defaultAction", "architectures", "syscalls") }).Count -gt 0) {
        Fail-Contract "backend '$BackendId' seccomp profile contains an unknown field"
    }
    if ([string]$profile.defaultAction -ne "SCMP_ACT_ERRNO") {
        Fail-Contract "backend '$BackendId' seccomp profile must default to SCMP_ACT_ERRNO"
    }
    $architectureProperty = $profile.PSObject.Properties["architectures"]
    if ($null -ne $architectureProperty) {
        if (!($profile.architectures -is [System.Array]) -or @($profile.architectures).Count -eq 0) {
            Fail-Contract "backend '$BackendId' seccomp profile architectures must be a non-empty array"
        }
        foreach ($architecture in @($profile.architectures)) {
            if ($architecture -isnot [string] -or [string]::IsNullOrWhiteSpace($architecture)) {
                Fail-Contract "backend '$BackendId' seccomp profile architectures must contain names"
            }
        }
    }
    $syscallsProperty = $profile.PSObject.Properties["syscalls"]
    if ($null -eq $syscallsProperty -or !($profile.syscalls -is [System.Array]) -or @($profile.syscalls).Count -eq 0) {
        Fail-Contract "backend '$BackendId' seccomp profile must contain a non-empty syscall allowlist"
    }
    $seenNames = @{}
    foreach ($group in @($profile.syscalls)) {
        if ($null -eq $group) {
            Fail-Contract "backend '$BackendId' seccomp syscall groups must be objects"
        }
        $groupFields = @($group.PSObject.Properties.Name)
        if (($groupFields | Where-Object { $_ -notin @("names", "action") }).Count -gt 0) {
            Fail-Contract "backend '$BackendId' seccomp syscall group contains an unknown field"
        }
        if ([string]$group.action -ne "SCMP_ACT_ALLOW") {
            Fail-Contract "backend '$BackendId' seccomp syscall groups must use SCMP_ACT_ALLOW"
        }
        if (!($group.names -is [System.Array]) -or @($group.names).Count -eq 0) {
            Fail-Contract "backend '$BackendId' seccomp syscall groups must contain names"
        }
        foreach ($name in @($group.names)) {
            if ($name -isnot [string] -or [string]::IsNullOrWhiteSpace($name)) {
                Fail-Contract "backend '$BackendId' seccomp syscall names must be unique and non-empty"
            }
            $nameText = $name
            if ($seenNames.ContainsKey($nameText)) {
                Fail-Contract "backend '$BackendId' seccomp syscall names must be unique and non-empty"
            }
            $seenNames[$nameText] = $true
        }
    }
}

function Require-Policy {
    param(
        [Parameter(Mandatory = $true)]$Registration,
        [Parameter(Mandatory = $true)][string]$BackendId
    )
    $policy = $Registration.policy
    if ($null -eq $policy) {
        Fail-Contract "backend '$BackendId' has no sandbox policy"
    }
    if ($policy.oci_privilege -ne "rootless") {
        Fail-Contract "backend '$BackendId' must use rootless OCI"
    }
    if ($policy.cgroup -ne "v2") {
        Fail-Contract "backend '$BackendId' must require cgroup v2"
    }
    if ($policy.privilege_escalation -ne "no_new_privileges") {
        Fail-Contract "backend '$BackendId' must require no_new_privileges"
    }
    if ($policy.root_filesystem -ne "read_only") {
        Fail-Contract "backend '$BackendId' must require a read-only root filesystem"
    }
    if ($policy.network -ne "deny_all") {
        Fail-Contract "backend '$BackendId' must deny network egress"
    }
    $namespaces = @($policy.namespaces | ForEach-Object { [string]$_ } | Sort-Object)
    $expectedNamespaces = @("mount", "network", "pid", "user")
    if (($namespaces -join ",") -ne ($expectedNamespaces -join ",")) {
        Fail-Contract "backend '$BackendId' must declare exactly user/pid/mount/network namespaces"
    }
    if ($null -eq $policy.seccomp -or $policy.seccomp.default_action -ne "default_deny") {
        Fail-Contract "backend '$BackendId' must provide a default-deny seccomp policy"
    }
    Require-Sha256 "backend '$BackendId' seccomp profile" ([string]$policy.seccomp.profile_sha256)
    if (@($policy.mounts).Count -eq 0) {
        Fail-Contract "backend '$BackendId' must declare explicit safe mounts"
    }
    foreach ($mount in @($policy.mounts)) {
        if ($mount.kind -eq "read_only_artifact") {
            if ([string]::IsNullOrWhiteSpace([string]$mount.artifact_id) -or
                [string]$mount.artifact_id -match '(^|[\\/])\.\.([\\/]|$)') {
                Fail-Contract "backend '$BackendId' has an unsafe artifact mount"
            }
        } elseif ($mount.kind -ne "ephemeral_scratch") {
            Fail-Contract "backend '$BackendId' has an unknown mount kind"
        }
    }
}

function Read-OperatorRegistry {
    param([Parameter(Mandatory = $true)][string]$Path)
    Require-AbsolutePath "production backend registry" $Path
    Require-RegularFile "production backend registry" $Path
    try {
        $parsed = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    } catch {
        Fail-Contract "production backend registry is not valid JSON: $Path"
    }
    $registrations = @($parsed)
    if ($registrations.Count -eq 0) {
        Fail-Contract "production backend registry is empty"
    }
    return $registrations
}

function Validate-OperatorRegistry {
    param([Parameter(Mandatory = $true)]$Registrations)
    $ids = @{}
    foreach ($registration in $Registrations) {
        $backendId = [string]$registration.backend_id
        if ([string]::IsNullOrWhiteSpace($backendId)) {
            Fail-Contract "operator registry contains an empty backend id"
        }
        if ($ids.ContainsKey($backendId)) {
            Fail-Contract "operator registry contains duplicate backend '$backendId'"
        }
        $ids[$backendId] = $true

        foreach ($field in @("bundle_root", "artifact_root", "runner_executable", "runner_state_root", "seccomp_profile_path")) {
            $value = [string]$registration.$field
            Require-AbsolutePath "backend '$backendId' $field" $value
        }
        Require-Directory "backend '$backendId' bundle root" ([string]$registration.bundle_root)
        Require-Directory "backend '$backendId' bundle rootfs" (Join-Path ([string]$registration.bundle_root) "rootfs")
        Require-Directory "backend '$backendId' artifact root" ([string]$registration.artifact_root)
        Require-Directory "backend '$backendId' runner state root" ([string]$registration.runner_state_root)
        Require-RegularFile "backend '$backendId' pinned runner" ([string]$registration.runner_executable)
        Require-Sha256 "backend '$backendId' runner" ([string]$registration.runner_sha256)
        $actualRunner = (Get-FileHash -LiteralPath ([string]$registration.runner_executable) -Algorithm SHA256).Hash.ToLowerInvariant()
        $expectedRunner = ([string]$registration.runner_sha256).Substring(7).ToLowerInvariant()
        if ($actualRunner -ne $expectedRunner) {
            Fail-Contract "backend '$backendId' runner SHA-256 does not match the operator pin"
        }
        if ([string]$registration.execution_mode -ne "production_sandboxed_oci") {
            Fail-Contract "backend '$backendId' is not a production_sandboxed_oci registration"
        }
        Require-Sha256 "backend '$backendId' guest image" ([string]$registration.guest_image_digest)
        Require-Policy $registration $backendId
        Require-SeccompProfile $registration $backendId
    }
    return $Registrations
}

function Invoke-ComposeConfigCheck {
    param(
        [Parameter(Mandatory = $true)][string]$ComposePath,
        [Parameter(Mandatory = $true)][string]$Project
    )
    Require-Command "docker"
    $configOutput = @(& docker compose --project-name $Project --project-directory $repoRoot --file $ComposePath config --format json 2>&1)
    if ($LASTEXITCODE -ne 0) {
        Fail-Contract "docker compose config failed; provide the required release secrets and operator environment"
    }
    try {
        $resolved = ($configOutput | ForEach-Object { $_.ToString() }) -join "`n" | ConvertFrom-Json
    } catch {
        Fail-Contract "docker compose config did not return valid JSON"
    }
    $resolvedVolumeNames = @(
        $resolved.volumes.PSObject.Properties |
            ForEach-Object { [string]$_.Value.name }
    )
    $unisolatedVolumeNames = @(
        $resolvedVolumeNames | Where-Object { !$_ -or !$_.StartsWith("$Project-") }
    )
    if ($resolvedVolumeNames.Count -eq 0 -or $unisolatedVolumeNames.Count -gt 0) {
        Fail-Contract "resolved isolated Compose volume names must be project-prefixed"
    }
}

function Get-FreeTcpPort {
    $listener = $null
    try {
        $listener = New-Object System.Net.Sockets.TcpListener(
            [System.Net.IPAddress]::Loopback,
            0
        )
        $listener.Start()
        return ([System.Net.IPEndPoint]$listener.LocalEndpoint).Port
    } catch {
        Fail-Contract "unable to reserve an isolated host TCP port: $($_.Exception.Message)"
    } finally {
        if ($null -ne $listener) {
            $listener.Stop()
        }
    }
}

function Require-TrueEvidenceCheck {
    param(
        [Parameter(Mandatory = $true)]$Checks,
        [Parameter(Mandatory = $true)][string]$Name
    )
    $property = $Checks.PSObject.Properties[$Name]
    if ($null -eq $property -or $property.Value -isnot [bool] -or !$property.Value) {
        Fail-Contract "reviewed OCI fixture evidence check '$Name' did not pass"
    }
}

function Validate-Evidence {
    param(
        [Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)][string]$Project,
        [Parameter(Mandatory = $true)][string]$ExpectedTaskId
    )
    if ($null -eq $Evidence) {
        Fail-Contract "reviewed OCI fixture did not produce an evidence object"
    }
    if ([string]$Evidence.schema_version -ne "general-compute-oci-e2e-v1") {
        Fail-Contract "reviewed OCI fixture evidence schema_version must be general-compute-oci-e2e-v1"
    }
    if ([string]$Evidence.project_name -ne $Project) {
        Fail-Contract "reviewed OCI fixture evidence is bound to a different Compose project"
    }
    if ([string]$Evidence.task_id -ne $ExpectedTaskId) {
        Fail-Contract "reviewed OCI fixture evidence is bound to a different task"
    }
    if ([string]$Evidence.status -ne "passed") {
        Fail-Contract "reviewed OCI fixture evidence status must be passed"
    }
    if ($null -eq $Evidence.services -or $null -eq $Evidence.checks) {
        Fail-Contract "reviewed OCI fixture evidence must contain services and checks objects"
    }
    foreach ($requiredService in @("postgres", "nodepool", "master", "worker")) {
        $serviceProperty = $Evidence.services.PSObject.Properties[$requiredService]
        if ($null -eq $serviceProperty -or [string]::IsNullOrWhiteSpace([string]$serviceProperty.Value)) {
            Fail-Contract "reviewed OCI fixture evidence must identify service '$requiredService'"
        }
    }
    foreach ($requiredCheck in @(
        "worker_registered",
        "task_completion",
        "postgres_settlement",
        "timeout_cancel",
        "network_denied",
        "filesystem_denied"
    )) {
        Require-TrueEvidenceCheck -Checks $Evidence.checks -Name $requiredCheck
    }
    if ($null -eq $Evidence.result -or
        [string]$Evidence.result.kind -ne "general-compute-result-v1" -or
        $Evidence.result.production_result_envelope -isnot [bool] -or
        !$Evidence.result.production_result_envelope) {
        Fail-Contract "reviewed OCI fixture must validate a ProductionResultEnvelope with kind general-compute-result-v1"
    }
    return $Evidence
}

function Invoke-ReviewedTaskFixture {
    param(
        [Parameter(Mandatory = $true)][string]$FixturePath,
        [Parameter(Mandatory = $true)][ValidateSet("provision", "execute")][string]$Phase,
        [Parameter(Mandatory = $true)][string]$ComposeProject,
        [Parameter(Mandatory = $true)][string]$ComposePath,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$EvidencePath,
        [Parameter(Mandatory = $true)][string]$TaskId,
        [Parameter(Mandatory = $true)][string]$MasterBaseUrl,
        [Parameter(Mandatory = $true)][string]$WorkerControlBaseUrl,
        [Parameter(Mandatory = $true)][string]$Username,
        [Parameter(Mandatory = $true)][string]$Password,
        [Parameter(Mandatory = $true)][string]$ConfigVolumeName,
        [Parameter(Mandatory = $true)][string]$StateVolumeName,
        [Parameter(Mandatory = $true)][string]$WorkerId
    )
    Require-RegularFile "reviewed OCI task fixture" $FixturePath
    if ([IO.Path]::GetExtension($FixturePath).ToLowerInvariant() -ne ".ps1") {
        Fail-Contract "reviewed OCI task fixture must be a PowerShell script with an explicit protocol"
    }
    if ($Phase -eq "execute" -and (Test-Path -LiteralPath $EvidencePath)) {
        Fail-Contract "reviewed OCI fixture evidence path already exists; refusing to overwrite it"
    }

    $powershell = Get-Command "pwsh" -ErrorAction SilentlyContinue
    if ($null -eq $powershell) {
        $powershell = Get-Command "powershell" -ErrorAction SilentlyContinue
    }
    if ($null -eq $powershell) {
        Fail-Contract "reviewed OCI task fixture requires pwsh or powershell"
    }
    $fixtureArguments = @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", $FixturePath,
        "-Phase", $Phase,
        "-ComposeProject", $ComposeProject,
        "-ComposeFile", $ComposePath,
        "-RegistryPath", $RegistryPath,
        "-EvidencePath", $EvidencePath,
        "-TaskId", $TaskId,
        "-MasterBaseUrl", $MasterBaseUrl,
        "-WorkerControlBaseUrl", $WorkerControlBaseUrl,
        "-Username", $Username,
        "-Password", $Password,
        "-ConfigVolumeName", $ConfigVolumeName,
        "-StateVolumeName", $StateVolumeName,
        "-WorkerId", $WorkerId
    )
    $fixtureOutput = @(& $powershell.Source @fixtureArguments 2>&1)
    $fixtureExitCode = $LASTEXITCODE
    if ($fixtureExitCode -ne 0) {
        $diagnostics = ($fixtureOutput | ForEach-Object { $_.ToString() }) -join "`n"
        Fail-Contract "reviewed OCI task fixture phase '$Phase' failed with exit code $fixtureExitCode`n$diagnostics"
    }
    if ($Phase -ne "execute") {
        return $null
    }
    Require-RegularFile "reviewed OCI fixture evidence" $EvidencePath
    try {
        $evidence = Get-Content -LiteralPath $EvidencePath -Raw -Encoding utf8 | ConvertFrom-Json
    } catch {
        Fail-Contract "reviewed OCI fixture evidence is not valid JSON"
    }
    return (Validate-Evidence -Evidence $evidence -Project $ComposeProject -ExpectedTaskId $TaskId)
}

$composePortEnvironmentNames = @(
    "POSTGRES_HOST_PORT",
    "REDIS_HOST_PORT",
    "NODEPOOL_GRPC_HOST_PORT",
    "TORRENT_TRACKER_HOST_PORT",
    "TORRENT_SEED_HOST_PORT",
    "MASTER_HTTP_HOST_PORT",
    "WORKER_GRPC_HOST_PORT",
    "WORKER_CONTROL_HOST_PORT"
)
$script:composePortEnvironmentBackup = [ordered]@{}
$script:composePortsApplied = $false

function Use-IsolatedComposePorts {
    if ($script:composePortsApplied) {
        Fail-Contract "isolated Compose host ports were applied more than once"
    }
    $usedPorts = @{}
    foreach ($name in $composePortEnvironmentNames) {
        $script:composePortEnvironmentBackup[$name] =
            [Environment]::GetEnvironmentVariable($name, "Process")
        do {
            $port = Get-FreeTcpPort
        } while ($usedPorts.ContainsKey($port))
        $usedPorts[$port] = $true
        [Environment]::SetEnvironmentVariable($name, [string]$port, "Process")
    }
    $script:composePortsApplied = $true
}

function Restore-IsolatedComposePorts {
    if (!$script:composePortsApplied) {
        return
    }
    foreach ($name in $composePortEnvironmentNames) {
        [Environment]::SetEnvironmentVariable(
            $name,
            $script:composePortEnvironmentBackup[$name],
            "Process"
        )
    }
    $script:composePortEnvironmentBackup = [ordered]@{}
    $script:composePortsApplied = $false
}

$composeFixtureEnvironmentNames = @(
    "HIVEMIND_SEED_DEFAULT_USER",
    "WORKER_NODEPOOL_USERNAME",
    "WORKER_NODEPOOL_PASSWORD",
    "WORKER_ID",
    "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_EVIDENCE"
)
$script:composeFixtureEnvironmentBackup = [ordered]@{}
$script:composeFixtureEnvironmentApplied = $false

function Use-ComposeFixtureEnvironment {
    param(
        [Parameter(Mandatory = $true)][string]$Username,
        [Parameter(Mandatory = $true)][string]$Password,
        [Parameter(Mandatory = $true)][string]$EvidencePath,
        [Parameter(Mandatory = $true)][string]$WorkerId
    )
    if ($script:composeFixtureEnvironmentApplied) {
        Fail-Contract "Compose fixture environment was applied more than once"
    }
    foreach ($name in $composeFixtureEnvironmentNames) {
        $script:composeFixtureEnvironmentBackup[$name] =
            [Environment]::GetEnvironmentVariable($name, "Process")
    }
    [Environment]::SetEnvironmentVariable("HIVEMIND_SEED_DEFAULT_USER", "1", "Process")
    [Environment]::SetEnvironmentVariable("WORKER_NODEPOOL_USERNAME", $Username, "Process")
    [Environment]::SetEnvironmentVariable("WORKER_NODEPOOL_PASSWORD", $Password, "Process")
    [Environment]::SetEnvironmentVariable("WORKER_ID", $WorkerId, "Process")
    [Environment]::SetEnvironmentVariable(
        "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_EVIDENCE",
        $EvidencePath,
        "Process"
    )
    $script:composeFixtureEnvironmentApplied = $true
}

function Restore-ComposeFixtureEnvironment {
    if (!$script:composeFixtureEnvironmentApplied) {
        return
    }
    foreach ($name in $composeFixtureEnvironmentNames) {
        [Environment]::SetEnvironmentVariable(
            $name,
            $script:composeFixtureEnvironmentBackup[$name],
            "Process"
        )
    }
    $script:composeFixtureEnvironmentBackup = [ordered]@{}
    $script:composeFixtureEnvironmentApplied = $false
}

$isolatedComposeVolumeEnvironmentNames = @(
    "REDIS_VOLUME_NAME",
    "POSTGRES_VOLUME_NAME",
    "HIVEMIND_DATA_VOLUME_NAME",
    "NODEPOOL_TORRENTS_VOLUME_NAME",
    "NODEPOOL_TASK_PACKAGES_VOLUME_NAME",
    "MASTER_TASK_REFERENCES_VOLUME_NAME",
    "MASTER_TORRENTS_VOLUME_NAME",
    "WORKER_TASK_DOWNLOADS_VOLUME_NAME",
    "WORKER_TORRENTS_VOLUME_NAME",
    "WORKER_GENERAL_COMPUTE_CONFIG_VOLUME_NAME",
    "WORKER_GENERAL_COMPUTE_STATE_VOLUME_NAME"
)
$script:isolatedComposeVolumeEnvironmentBackup = [ordered]@{}
$script:isolatedComposeVolumesApplied = $false

function Use-IsolatedComposeVolumes {
    param([Parameter(Mandatory = $true)][string]$Project)
    if ($script:isolatedComposeVolumesApplied) {
        Fail-Contract "isolated Compose volume names were applied more than once"
    }
    foreach ($name in $isolatedComposeVolumeEnvironmentNames) {
        $script:isolatedComposeVolumeEnvironmentBackup[$name] =
            [Environment]::GetEnvironmentVariable($name, "Process")
        [Environment]::SetEnvironmentVariable(
            $name,
            "$Project-$($name.ToLowerInvariant())",
            "Process"
        )
    }
    $script:isolatedComposeVolumesApplied = $true
}

function Restore-IsolatedComposeVolumes {
    if (!$script:isolatedComposeVolumesApplied) {
        return
    }
    foreach ($name in $isolatedComposeVolumeEnvironmentNames) {
        [Environment]::SetEnvironmentVariable(
            $name,
            $script:isolatedComposeVolumeEnvironmentBackup[$name],
            "Process"
        )
    }
    $script:isolatedComposeVolumeEnvironmentBackup = [ordered]@{}
    $script:isolatedComposeVolumesApplied = $false
}

if ($CheckOnly -and $Run) {
    Fail-Contract "-CheckOnly and -Run are mutually exclusive"
}
if (!$CheckOnly -and !$Run) {
    $CheckOnly = $true
}

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($RegistryPath)) {
    $RegistryPath = $env:HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS
}
if ([string]::IsNullOrWhiteSpace($RegistryPath)) {
    Fail-Contract "set HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS to the operator registry (the Worker path is /etc/hivemind/general-compute/backends.json)"
}
if ([string]::IsNullOrWhiteSpace($ComposeFile)) {
    $ComposeFile = Join-Path $repoRoot "docker-compose.yml"
}
Require-RegularFile "release Compose file" $ComposeFile

$registrations = Validate-OperatorRegistry (Read-OperatorRegistry $RegistryPath)
$safeProjectName = "$ProjectName-$([guid]::NewGuid().ToString('N').Substring(0, 12))"
try {
    Use-IsolatedComposeVolumes -Project $safeProjectName
    Invoke-ComposeConfigCheck -ComposePath $ComposeFile -Project $safeProjectName

    Write-Host ("CHECK operator OCI registry: {0} backend(s)" -f @($registrations).Count)
    Write-Host "CHECK rootless policy, pinned runner/rootfs, and seccomp digest: PASS"
    Write-Host "CHECK isolated Compose project '$safeProjectName': PASS"

    if ($CheckOnly) {
        Write-Host "general-compute OCI E2E preflight passed (execution not requested)"
        return
    }

    if ($env:HIVEMIND_ENABLE_REAL_OCI_E2E -ne "1") {
        Fail-Contract "-Run requires HIVEMIND_ENABLE_REAL_OCI_E2E=1; no implicit deployment execution is allowed"
    }

    $fixturePath = $env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_TASK_FIXTURE
    if ([string]::IsNullOrWhiteSpace($fixturePath)) {
        Fail-Contract "-Run requires HIVEMIND_GENERAL_COMPUTE_OCI_E2E_TASK_FIXTURE with a Postgres-backed task fixture"
    }
    Require-RegularFile "multi-process OCI task fixture" $fixturePath

    $fixtureUsername = [string]$env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_USERNAME
    if ([string]::IsNullOrWhiteSpace($fixtureUsername)) {
        $fixtureUsername = "testuser"
    }
    $fixturePassword = [string]$env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_PASSWORD
    if ([string]::IsNullOrWhiteSpace($fixturePassword)) {
        $fixturePassword = "testpass123"
    }
    $fixtureWorkerId = [string]$env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_WORKER_ID
    if ([string]::IsNullOrWhiteSpace($fixtureWorkerId)) {
        $fixtureWorkerId = "oci-e2e-worker"
    }
    if ($fixtureWorkerId -notmatch '^[A-Za-z0-9_.-]+$') {
        Fail-Contract "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_WORKER_ID must be a safe worker id"
    }
    $fixtureTaskId = "oci-e2e-$([guid]::NewGuid().ToString('N'))"
    $evidencePath = [string]$env:HIVEMIND_GENERAL_COMPUTE_OCI_E2E_EVIDENCE
    if ([string]::IsNullOrWhiteSpace($evidencePath)) {
        $evidenceDirectory = Join-Path $repoRoot "test_logs"
        if (Test-Path -LiteralPath $evidenceDirectory -PathType Container) {
            Require-Directory "OCI E2E evidence directory" $evidenceDirectory
        } else {
            New-Item -ItemType Directory -Path $evidenceDirectory -Force | Out-Null
        }
        $evidencePath = Join-Path $evidenceDirectory (
            "general-compute-oci-e2e-" + $safeProjectName + ".json"
        )
    } else {
        Require-AbsolutePath "OCI E2E evidence" $evidencePath
        $evidenceParent = Split-Path -Parent $evidencePath
        Require-Directory "OCI E2E evidence parent" $evidenceParent
    }
    if (Test-Path -LiteralPath $evidencePath) {
        Fail-Contract "OCI E2E evidence path already exists; choose a new path to avoid overwriting prior evidence"
    }
    $configVolumeName = "$safeProjectName-worker_general_compute_config_volume_name"
    $stateVolumeName = "$safeProjectName-worker_general_compute_state_volume_name"
    $composeStarted = $false
    try {
        Use-IsolatedComposePorts
        Use-ComposeFixtureEnvironment `
            -Username $fixtureUsername `
            -Password $fixturePassword `
            -EvidencePath $evidencePath `
            -WorkerId $fixtureWorkerId
        Invoke-ComposeConfigCheck -ComposePath $ComposeFile -Project $safeProjectName

        # Provisioning is part of the reviewed fixture contract: it copies the
        # operator registry/rootfs/runner/profile into the project-prefixed
        # named volumes and rewrites only those operator-owned paths to the
        # fixed in-container locations consumed by the Worker.
        $composeStarted = $true
        Invoke-ReviewedTaskFixture `
            -FixturePath $fixturePath `
            -Phase provision `
            -ComposeProject $safeProjectName `
            -ComposePath $ComposeFile `
            -RegistryPath $RegistryPath `
            -EvidencePath $evidencePath `
            -TaskId $fixtureTaskId `
            -MasterBaseUrl "http://127.0.0.1:$([Environment]::GetEnvironmentVariable('MASTER_HTTP_HOST_PORT', 'Process'))" `
            -WorkerControlBaseUrl "http://127.0.0.1:$([Environment]::GetEnvironmentVariable('WORKER_CONTROL_HOST_PORT', 'Process'))" `
            -Username $fixtureUsername `
            -Password $fixturePassword `
            -ConfigVolumeName $configVolumeName `
            -StateVolumeName $stateVolumeName `
            -WorkerId $fixtureWorkerId

        Write-Host "RUN docker compose up -d --build (postgres redis nodepool master worker)"
        & docker compose `
            --project-name $safeProjectName `
            --project-directory $repoRoot `
            --file $ComposeFile `
            up -d --build postgres redis nodepool master worker
        if ($LASTEXITCODE -ne 0) {
            Fail-Contract "docker compose up failed for isolated project '$safeProjectName'"
        }

        $evidence = Invoke-ReviewedTaskFixture `
            -FixturePath $fixturePath `
            -Phase execute `
            -ComposeProject $safeProjectName `
            -ComposePath $ComposeFile `
            -RegistryPath $RegistryPath `
            -EvidencePath $evidencePath `
            -TaskId $fixtureTaskId `
            -MasterBaseUrl "http://127.0.0.1:$([Environment]::GetEnvironmentVariable('MASTER_HTTP_HOST_PORT', 'Process'))" `
            -WorkerControlBaseUrl "http://127.0.0.1:$([Environment]::GetEnvironmentVariable('WORKER_CONTROL_HOST_PORT', 'Process'))" `
            -Username $fixtureUsername `
            -Password $fixturePassword `
            -ConfigVolumeName $configVolumeName `
            -StateVolumeName $stateVolumeName `
            -WorkerId $fixtureWorkerId
        Write-Host ("general-compute OCI E2E fixture passed: task {0}, evidence {1}" -f $evidence.task_id, $evidencePath)
    } finally {
        if ($composeStarted) {
            Write-Host "CLEANUP docker compose down --volumes --remove-orphans"
            & docker compose `
                --project-name $safeProjectName `
                --project-directory $repoRoot `
                --file $ComposeFile `
                down --volumes --remove-orphans
            if ($LASTEXITCODE -ne 0) {
                Fail-Contract "isolated Compose cleanup failed for project '$safeProjectName'"
            }
            $remaining = @(& docker compose `
                --project-name $safeProjectName `
                --project-directory $repoRoot `
                --file $ComposeFile `
                ps -q 2>$null)
            if ($LASTEXITCODE -eq 0 -and $remaining.Count -gt 0) {
                Fail-Contract "isolated Compose cleanup left running containers for project '$safeProjectName'"
            }
        }
        Restore-ComposeFixtureEnvironment
        Restore-IsolatedComposePorts
    }
} finally {
    Restore-IsolatedComposeVolumes
}
