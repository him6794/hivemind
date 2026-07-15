param(
  [string]$MasterUrl = 'http://127.0.0.1:8082',
  [Parameter(Mandatory=$true)][string]$Token,
  [int]$TaskCount = 10,
  [int]$TimeoutSeconds = 180,
  [int]$PollSeconds = 1
)
$ErrorActionPreference = 'Stop'
$headers = @{ Authorization = "Bearer $Token" }
$src = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.hmf')
$inputJson = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.input.json')
$runId = Get-Date -Format 'yyyyMMddHHmmss'
$taskIds = 1..$TaskCount | ForEach-Object { "stress-$runId-$_" }
$sw = [Diagnostics.Stopwatch]::StartNew()
$submitLat = @()
foreach ($taskId in $taskIds) {
  $body = (@{
    task_id = $taskId
    runtime = 'managed-function-v0'
    task_source = $src
    torrent = $inputJson
    location = 'local'
    host_count = 1
    max_cpt = 100
  } | ConvertTo-Json -Compress -Depth 8)
  $s = [Diagnostics.Stopwatch]::StartNew()
  $resp = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks" -Headers $headers -ContentType 'application/json' -Body $body -TimeoutSec 30
  $s.Stop()
  if (-not $resp.success) { throw "submit failed for $taskId : $($resp.message)" }
  $submitLat += $s.ElapsedMilliseconds
}
$pending = [System.Collections.Generic.HashSet[string]]::new([string[]]$taskIds)
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$statusMap = @{}
while ($pending.Count -gt 0 -and (Get-Date) -lt $deadline) {
  Start-Sleep -Seconds $PollSeconds
  $tasks = @( (Invoke-RestMethod -Uri "$MasterUrl/api/tasks" -Headers $headers).tasks )
  foreach ($id in @($pending)) {
    $t = $tasks | Where-Object { $_.task_id -eq $id } | Select-Object -First 1
    if ($t -and $t.status -in @('COMPLETED','FAILED','TIMED_OUT','CANCELLED')) {
      $statusMap[$id] = $t.status
      [void]$pending.Remove($id)
    }
  }
}
$sw.Stop()
$completed = ($statusMap.Values | Where-Object { $_ -eq 'COMPLETED' }).Count
$failed = $TaskCount - $completed
[pscustomobject]@{
  task_count = $TaskCount
  completed = $completed
  failed_or_pending = $failed + $pending.Count
  pending = $pending.Count
  total_ms = $sw.ElapsedMilliseconds
  avg_submit_ms = [math]::Round(($submitLat | Measure-Object -Average).Average, 2)
  max_submit_ms = ($submitLat | Measure-Object -Maximum).Maximum
  statuses = ($statusMap.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ';'
} | ConvertTo-Json -Depth 4
if ($completed -ne $TaskCount) { exit 1 }
