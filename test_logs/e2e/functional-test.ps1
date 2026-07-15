param(
  [string]$MasterUrl = 'http://127.0.0.1:8082',
  [int]$TimeoutSeconds = 90
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$results = [System.Collections.ArrayList]::new()
function Add-R([string]$name,[bool]$pass,[string]$detail) {
  [void]$results.Add([pscustomobject]@{ check=$name; pass=$pass; detail=$detail })
  Write-Output ("[{0}] {1} : {2}" -f $(if($pass){'PASS'}else{'FAIL'}),$name,$detail)
}

# 1. Login
$lb = @{ username='testuser'; password='testpass123' } | ConvertTo-Json -Compress
$lr = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/login" -ContentType 'application/json' -Body $lb -TimeoutSec 10
$token = $lr.token
Add-R 'login' ($lr.success -and $token) "success=$($lr.success) tokenLen=$($token.Length)"
$h = @{ Authorization = "Bearer $token" }

# 2. List workers (simulating user opening worker panel)
$w = Invoke-RestMethod -Uri "$MasterUrl/api/workers" -Headers $h -TimeoutSec 10
$idle = @($w.workers | Where-Object { $_.status -eq 'IDLE' }).Count
Add-R 'list_workers' ($w.workers.Count -ge 1 -and $idle -ge 1) "count=$($w.workers.Count) idle=$idle"

# 3. Submit managed-function task (simulating user submitting a job)
$src = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.hmf')
$inJson = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.input.json')
$taskId = "func-$(Get-Date -Format yyyyMMddHHmmss)"
$body = (@{ task_id=$taskId; runtime='managed-function-v0'; task_source=$src; torrent=$inJson; location='local'; host_count=1; max_cpt=100 } | ConvertTo-Json -Compress -Depth 8)
$create = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks" -Headers $h -ContentType 'application/json' -Body $body -TimeoutSec 30
Add-R 'submit_task' $create.success "task=$taskId success=$($create.success)"

# 4. Poll until terminal
$deadline = (Get-Date).AddSeconds($TimeoutSeconds)
$status='PENDING'
while ((Get-Date) -lt $deadline) {
  Start-Sleep -Seconds 1
  $tasks = (Invoke-RestMethod -Uri "$MasterUrl/api/tasks" -Headers $h -TimeoutSec 10).tasks
  $t = $tasks | Where-Object { $_.task_id -eq $taskId } | Select-Object -First 1
  if ($t) { $status = $t.status; if ($status -in @('COMPLETED','FAILED','TIMED_OUT','CANCELLED')) { break } }
}
Add-R 'task_completed' ($status -eq 'COMPLETED') "task=$taskId status=$status"

# 5. Task result
try { $r = Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$taskId/result" -Headers $h -TimeoutSec 10; Add-R 'task_result' ($r.success) "success=$($r.success) status=$($r.status)" } catch { Add-R 'task_result' $false "ERR $($_.Exception.Message)" }

# 6. Task log
try { $lg = Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$taskId/log" -Headers $h -TimeoutSec 10; Add-R 'task_log' ($lg.success) "success=$($lg.success) logLen=$($lg.log.Length)" } catch { Add-R 'task_log' $false "ERR $($_.Exception.Message)" }

# 7. Balance
try { $bal = Invoke-RestMethod -Uri "$MasterUrl/api/balance" -Headers $h -TimeoutSec 10; Add-R 'balance' ($bal.success) "success=$($bal.success) balance=$($bal.balance)" } catch { Add-R 'balance' $false "ERR $($_.Exception.Message)" }

# 8. Admin billing overview (testuser is admin)
try { $ab = Invoke-RestMethod -Uri "$MasterUrl/api/admin/billing/overview" -Headers $h -TimeoutSec 10; Add-R 'admin_billing' ($ab.success) "success=$($ab.success)" } catch { Add-R 'admin_billing' $false "ERR $($_.Exception.Message)" }

# 9. Stop/cancel a task (submit then immediately stop)
$cancelId = "cancel-$(Get-Date -Format yyyyMMddHHmmss)"
$cb = @{ task_id=$cancelId; runtime='managed-function-v0'; task_source=$src; torrent=$inJson; location='local'; host_count=1; max_cpt=100 } | ConvertTo-Json -Compress -Depth 8
[void](Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks" -Headers $h -ContentType 'application/json' -Body $cb -TimeoutSec 30)
try { $stop = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks/$cancelId/stop" -Headers $h -TimeoutSec 10; Add-R 'stop_task' ($stop.success) "success=$($stop.success) msg=$($stop.message)" } catch { Add-R 'stop_task' $false "ERR $($_.Exception.Message)" }
Start-Sleep -Seconds 2
$ct = (Invoke-RestMethod -Uri "$MasterUrl/api/tasks" -Headers $h -TimeoutSec 10).tasks | Where-Object { $_.task_id -eq $cancelId } | Select-Object -First 1
Add-R 'task_cancelled' ($ct.status -eq 'CANCELLED') "task=$cancelId status=$($ct.status)"

# 10. Master UI
$mu = Invoke-WebRequest -Uri "$MasterUrl/" -UseBasicParsing -TimeoutSec 10
Add-R 'master_ui' ($mu.StatusCode -eq 200) "status=$($mu.StatusCode)"

# 11. Worker UI (served by worker process in-process)
$wu = Invoke-WebRequest -Uri 'http://127.0.0.1:18080/' -UseBasicParsing -TimeoutSec 10
Add-R 'worker_ui' ($wu.StatusCode -eq 200) "status=$($wu.StatusCode)"

$passed = ($results | Where-Object { $_.pass }).Count
$failed = ($results | Where-Object { -not $_.pass }).Count
Write-Output ""
Write-Output "FUNCTIONAL SUMMARY: $passed passed, $failed failed"
$results | ConvertTo-Json -Depth 4
if ($failed -gt 0) { exit 1 }


Write-Output "TEST_DONE_functional-test"
