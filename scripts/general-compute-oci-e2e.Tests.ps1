$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$harnessPath = Join-Path $PSScriptRoot "general-compute-oci-e2e.ps1"

if (!(Test-Path -LiteralPath $harnessPath -PathType Leaf)) {
    throw "Missing operator OCI E2E harness at $harnessPath."
}

$harnessText = Get-Content -LiteralPath $harnessPath -Raw
$composePath = Join-Path $repoRoot "docker-compose.yml"
if (!(Test-Path -LiteralPath $composePath -PathType Leaf)) {
    throw "OCI E2E harness requires the release Compose file at $composePath."
}
$composeText = Get-Content -LiteralPath $composePath -Raw
$fixtureTemplatePath = Join-Path $PSScriptRoot "general-compute-oci-task-fixture.ps1"

foreach ($expected in @(
    "param(",
    "[switch]`$CheckOnly",
    "[switch]`$Run",
    "HIVEMIND_ENABLE_REAL_OCI_E2E",
    "HIVEMIND_GENERAL_COMPUTE_PRODUCTION_BACKENDS",
    "runner_state_root",
    "seccomp_profile_path",
    "docker compose",
    "--project-name",
    "--project-directory",
    "finally",
    "RunnerNotPinned",
    "rootless",
    "seccomp",
    "SCMP_ACT_ERRNO",
    "SCMP_ACT_ALLOW",
    "syscalls",
    "Get-FileHash -LiteralPath",
    "-isnot [string]",
    "Use-IsolatedComposeVolumes",
    "Restore-IsolatedComposeVolumes",
    "WORKER_GENERAL_COMPUTE_CONFIG_VOLUME_NAME",
    "WORKER_GENERAL_COMPUTE_STATE_VOLUME_NAME",
    "HIVEMIND_GENERAL_COMPUTE_BACKENDS",
    "HIVEMIND_GENERAL_COMPUTE_WORKER_CAPABILITIES",
    "resolved isolated Compose volume names"
)) {
    if (!$harnessText.Contains($expected)) {
        throw "OCI E2E harness must contain the fail-closed/operator-isolation contract '$expected'."
    }
}

# The execution phase is an explicit reviewed-fixture protocol.  A fixture is
# not allowed to be an opaque command that merely exits zero: it must receive
# the isolated Compose identity and write a versioned evidence document that
# the harness validates before reporting an E2E pass.
foreach ($expected in @(
    "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_EVIDENCE",
    "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES",
    "-ComposeProject",
    "-ComposeFile",
    "-RegistryPath",
    "-EvidencePath",
    "Invoke-ReviewedTaskFixture",
    "docker compose",
    "up",
    "-d",
    "--build",
    "schema_version",
    "general-compute-oci-e2e-v1",
    "task_completion",
    "postgres_settlement",
    "timeout_cancel",
    "network_denied",
    "filesystem_denied",
    "worker_registered",
    "ProductionResultEnvelope"
)) {
    if (!$harnessText.Contains($expected)) {
        throw "OCI E2E execution must implement the reviewed fixture/evidence contract '$expected'."
    }
}

$casePlanGuardIndex = $harnessText.IndexOf('Require-RegularFile "OCI E2E case plan"')
$composeUpIndex = $harnessText.IndexOf('up -d --build')
if ($casePlanGuardIndex -lt 0 -or $composeUpIndex -lt 0 -or $casePlanGuardIndex -gt $composeUpIndex) {
    throw "OCI E2E -Run must validate the operator case plan before starting Compose."
}

if ($harnessText -match '(?i)multi-process task fixture execution is not yet wired') {
    throw "OCI E2E -Run must invoke the reviewed fixture instead of retaining the placeholder fail-closed branch."
}

if (!(Test-Path -LiteralPath $fixtureTemplatePath -PathType Leaf)) {
    throw "Repository must ship the reviewed Postgres-backed OCI fixture implementation at $fixtureTemplatePath."
}
$fixtureTemplateText = Get-Content -LiteralPath $fixtureTemplatePath -Raw
foreach ($expected in @(
    'ValidateSet("provision", "execute")',
    "HIVEMIND_GENERAL_COMPUTE_OCI_E2E_CASES",
    "Invoke-RestMethod",
    "general_compute_results",
    "general_compute_settlements",
    "encode(result_json",
    "general-compute-result-v1",
    "timeout_cancel",
    "network_denied",
    "filesystem_denied",
    "/state/bundles"
)) {
    if (!$fixtureTemplateText.Contains($expected)) {
        throw "Reviewed OCI fixture must contain the real multi-process evidence path '$expected'."
    }
}

if ($harnessText -notmatch '(?i)operator.*registry|registry.*operator') {
    throw "OCI E2E harness must describe the registry as operator-owned."
}
if ($harnessText -notmatch '(?i)rootfs') {
    throw "OCI E2E harness must validate an operator-provisioned rootfs."
}
if ($harnessText -notmatch '(?i)sha256') {
    throw "OCI E2E harness must verify pinned SHA-256 material."
}
if ($harnessText -notmatch '(?i)cleanup|down') {
    throw "OCI E2E harness must clean up its isolated Compose project."
}
if ($harnessText -match '(?i)fallback.*direct|direct.*fallback') {
    throw "OCI E2E harness must not offer a direct-process fallback."
}
if ($harnessText -match '(?i)MONTY_EXECUTABLE|/app/monty') {
    throw "OCI E2E harness must not reintroduce the removed Monty contract."
}
if ($composeText -match '(?m)^\s*container_name\s*:') {
    throw "Release Compose must not fix container_name values; project names must isolate OCI E2E containers."
}
if ($composeText -match '(?m)^\s*name\s*:\s*hivemind-network\s*$') {
    throw "Release Compose must not fix the network name; project names must isolate OCI E2E networks."
}
if ($composeText -match '(?m)^\s*ipv4_address\s*:') {
    throw "Release Compose must not fix service IPv4 addresses; project networks must allocate isolated addresses."
}
if ($composeText -match '(?m)^\s*-\s*subnet\s*:') {
    throw "Release Compose must not fix a subnet; project networks must allocate isolated address ranges."
}

Write-Host "general-compute OCI E2E harness contract passed"
