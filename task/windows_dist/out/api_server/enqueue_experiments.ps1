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
