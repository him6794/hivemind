# Creates Windows-friendly distribution folders for API server and worker client.
# Output:
#   task\windows_dist\out\api_server\
#   task\windows_dist\out\worker_client\

$ErrorActionPreference = "Stop"

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\.."))
$TaskDir  = Join-Path $RepoRoot "task"

$OutRoot  = Join-Path $PSScriptRoot "out"
$ApiOut   = Join-Path $OutRoot "api_server"
$WorkerOut= Join-Path $OutRoot "worker_client"

Write-Host "RepoRoot: $RepoRoot"
Write-Host "TaskDir : $TaskDir"
Write-Host "OutRoot : $OutRoot"

# Clean output
if (Test-Path $OutRoot) {
  Remove-Item -Recurse -Force $OutRoot
}
New-Item -ItemType Directory -Force -Path $ApiOut | Out-Null
New-Item -ItemType Directory -Force -Path $WorkerOut | Out-Null

# ---- API server package ----
Copy-Item -Force (Join-Path $TaskDir "api.py") (Join-Path $ApiOut "api.py")

$ApiTemplatesOut = Join-Path $ApiOut "templates"
New-Item -ItemType Directory -Force -Path $ApiTemplatesOut | Out-Null
Copy-Item -Force (Join-Path $TaskDir "templates\index.html") (Join-Path $ApiTemplatesOut "index.html")

@"
Flask
"@ | Set-Content -Encoding UTF8 (Join-Path $ApiOut "requirements.txt")

@'
# Run API server (Windows)
$ErrorActionPreference = 'Stop'

python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe api.py
'@ | Set-Content -Encoding UTF8 (Join-Path $ApiOut "run_server.ps1")

# Enqueue the full experiment grid
@'
# Enqueue the full experiment grid
# - repeats: 50
# - n: 5..12
# - workers: 1,10,20,30,40
# - chunks: 50,100,150,200,250
#
# Usage:
#   .\enqueue_experiments.ps1 -ServerUrl http://127.0.0.1:5001
param(
  [Parameter(Mandatory=$true)][string]$ServerUrl,
  [switch]$ClearQueue
)

$ErrorActionPreference = 'Stop'

$payload = @{
  n_values      = @(5,6,7,8,9,10,11,12)
  worker_values = @(1,10,20,30,40)
  chunk_values  = @(50,100,150,200,250)
  repeats       = 50
  clear_queue   = [bool]$ClearQueue
}

$uri = ($ServerUrl.TrimEnd('/') + '/enqueue_experiment_grid')

Write-Host "POST $uri"
Write-Host ("Total runs: " + ($payload.n_values.Count * $payload.worker_values.Count * $payload.chunk_values.Count * $payload.repeats))

Invoke-RestMethod -Method Post -Uri $uri -ContentType 'application/json' -Body ($payload | ConvertTo-Json -Depth 5)
'@ | Set-Content -Encoding UTF8 (Join-Path $ApiOut "enqueue_experiments.ps1")

# ---- Worker client package ----
Copy-Item -Force (Join-Path $TaskDir "main.py") (Join-Path $WorkerOut "main.py")

@"
requests
"@ | Set-Content -Encoding UTF8 (Join-Path $WorkerOut "requirements.txt")

@'
# Run worker client (Windows)
# Usage:
#   .\run_worker.ps1 -ServerUrl http://<API_SERVER_IP>:5001
param(
  [Parameter(Mandatory=$true)][string]$ServerUrl
)

$ErrorActionPreference = 'Stop'

python -m venv .venv
.\.venv\Scripts\pip.exe install -r requirements.txt
.\.venv\Scripts\python.exe main.py $ServerUrl
'@ | Set-Content -Encoding UTF8 (Join-Path $WorkerOut "run_worker.ps1")

# Copy DLLs if present
$DllCandidates = @(
  (Join-Path $TaskDir "prime_sieve.dll"),
  (Join-Path $TaskDir "prime_sieve.so")
)
foreach ($dll in $DllCandidates) {
  if (Test-Path $dll) {
    Copy-Item -Force $dll (Join-Path $WorkerOut (Split-Path $dll -Leaf))
  }
}

# Common OpenMP runtime location (optional)
$Libgomp = Join-Path $TaskDir "libgomp-1.dll"
if (Test-Path $Libgomp) {
  Copy-Item -Force $Libgomp (Join-Path $WorkerOut "libgomp-1.dll")
}

Write-Host "Done. Output created:" -ForegroundColor Green
Write-Host "  $ApiOut"
Write-Host "  $WorkerOut"
Write-Host "Note: Ensure worker_client contains prime_sieve.dll and any dependent DLLs." -ForegroundColor Yellow
