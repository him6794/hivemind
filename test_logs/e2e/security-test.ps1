param([string]$MasterUrl = 'http://127.0.0.1:8082')
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$results = [System.Collections.ArrayList]::new()
function Add-R([string]$name,[bool]$pass,[string]$detail) {
  [void]$results.Add([pscustomobject]@{ check=$name; pass=$pass; detail=$detail })
  Write-Output ("[{0}] {1} : {2}" -f $(if($pass){'PASS'}else{'FAIL'}),$name,$detail)
}
# Try-Req returns the parsed body in `resp` on HTTP 2xx (where the backend
# signals authorization denial via success:false), and `ed` (ErrorDetails) on
# 4xx/5xx. Tests must read `resp` for the 200-success:false contract.
function Try-Req([scriptblock]$block) {
  try { $r = & $block; return @{ ok=$true; status=200; resp=$r } }
  catch { $code = $_.Exception.Response.StatusCode.value__ ; return @{ ok=$false; status=$code; err=$_.Exception.Message; ed=$_.ErrorDetails.Message } }
}

# --- Ensure user2 exists (non-admin) ---
try { Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/register" -ContentType 'application/json' -Body '{"username":"user2","password":"user2pass123"}' -TimeoutSec 10 | Out-Null } catch {}
$l2 = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/login" -ContentType 'application/json' -Body '{"username":"user2","password":"user2pass123"}' -TimeoutSec 10
$u2 = @{ Authorization = "Bearer $($l2.token)" }
Add-R 'user2_login' $l2.success "success=$($l2.success)"

# --- Admin (testuser) context ---
$lr = Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/login" -ContentType 'application/json' -Body '{"username":"testuser","password":"testpass123"}' -TimeoutSec 10
$adm = @{ Authorization = "Bearer $($lr.token)" }

# 1-3. Unauthenticated access -> 401
$r1 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks" -TimeoutSec 10 }
Add-R 'unauth_tasks_401' ($r1.status -eq 401) "status=$($r1.status)"
$r2 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/workers" -TimeoutSec 10 }
Add-R 'unauth_workers_401' ($r2.status -eq 401) "status=$($r2.status)"
$r3 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/admin/billing/overview" -TimeoutSec 10 }
Add-R 'unauth_admin_401' ($r3.status -eq 401) "status=$($r3.status)"

# 4. Bad login -> 401, success=false
$r4 = Try-Req { Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/login" -ContentType 'application/json' -Body '{"username":"testuser","password":"wrong"}' -TimeoutSec 10 }
$bad = if ($r4.ed) { $r4.ed } else { '' }
Add-R 'bad_login_401' ($r4.status -eq 401 -and $bad -match '"success":false') "status=$($r4.status) body=$bad"

# 5. Unsafe task_id -> 400. URL-encoded variants bypass client-side URL
# normalization and actually reach the handler guard (is_safe_task_id); these
# prove the backend rejects path traversal with 400 + success:false.
foreach ($pair in @(@('dotdot','%2e%2e'),@('dotdot-slash','%2e%2e%2f'),@('slash','%2f'),@('dot','%2e'))) {
  $pairName=$pair[0]; $enc=$pair[1]
  $rr = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$enc/result" -Headers $adm -TimeoutSec 10 }
  $ok = ($rr.status -eq 400) -and ($rr.ed -match '"success":false')
  Add-R ("safe_taskid_enc_" + $pairName) $ok "enc=$enc status=$($rr.status) body=$($rr.ed)"
}
# Literal (un-encoded) traversal is normalized away by the HTTP client before
# routing; it must never reach the filesystem. 400 (handler) or 404 (no route)
# are both acceptable safety outcomes.
foreach ($tid in @('../evil','a/b','.')) {
  $rr = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$tid/result" -Headers $adm -TimeoutSec 10 }
  $ok = ($rr.status -eq 400) -or ($rr.status -eq 404)
  Add-R ("safe_taskid_lit_" + ($tid -replace '[^a-zA-Z0-9]','_')) $ok "tid=$tid status=$($rr.status)"
}

# 6. Cross-user task isolation: admin submits, user2 must NOT see it in their list
$src = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.hmf')
$inJson = [System.IO.File]::ReadAllText('D:\hivemind\templates\managed-function-v0\03_batch_sum.input.json')
$isoTaskId = "sec-iso-$(Get-Date -Format yyyyMMddHHmmss)"
$acb = @{ task_id=$isoTaskId; runtime='managed-function-v0'; task_source=$src; torrent=$inJson; location='local'; host_count=1; max_cpt=100 } | ConvertTo-Json -Compress -Depth 8
[void](Invoke-RestMethod -Method Post -Uri "$MasterUrl/api/tasks" -Headers $adm -ContentType 'application/json' -Body $acb -TimeoutSec 30)
$u2tasks = (Invoke-RestMethod -Uri "$MasterUrl/api/tasks" -Headers $u2 -TimeoutSec 10).tasks
$visible = @($u2tasks | Where-Object { $_.task_id -eq $isoTaskId }).Count
Add-R 'cross_user_isolation' ($visible -eq 0) "user2 sees admin task count=$visible (expect 0)"

# 7. Cross-user result must be DENIED. Backend returns HTTP 200 with
# success:false + status_message:"Not authorized". Body is in `resp`.
$rr9 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$isoTaskId/result" -Headers $u2 -TimeoutSec 10 }
$resp9 = if ($rr9.resp) { $rr9.resp | ConvertTo-Json -Compress -Depth 4 } else { '' }
$resultDenied = (($rr9.status -eq 200) -and ($resp9 -match '"success":false')) -or ($rr9.status -eq 401) -or ($rr9.status -eq 403)
Add-R 'cross_user_result_denied' $resultDenied "status=$($rr9.status) body=$resp9"

# 8. Cross-user log must be DENIED (same success:false contract).
$rr10 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$isoTaskId/log" -Headers $u2 -TimeoutSec 10 }
$resp10 = if ($rr10.resp) { $rr10.resp | ConvertTo-Json -Compress -Depth 4 } else { '' }
$logDenied = (($rr10.status -eq 200) -and ($resp10 -match '"success":false')) -or ($rr10.status -eq 401) -or ($rr10.status -eq 403)
Add-R 'cross_user_log_denied' $logDenied "status=$($rr10.status) body=$resp10"

# 9. Non-admin admin billing -> success:false (denied, zeroed values)
$ab2 = Invoke-RestMethod -Uri "$MasterUrl/api/admin/billing/overview" -Headers $u2 -TimeoutSec 10
$b11 = $ab2 | ConvertTo-Json -Compress -Depth 4
Add-R 'nonadmin_billing_denied' ($ab2.success -eq $false) "body=$b11"

# 10. Non-admin admin audit logs -> success:false (denied). Real route is
# /api/admin/audit/logs; denial is signalled by resp.success=false (HTTP 200).
$rl2 = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/admin/audit/logs?limit=5" -Headers $u2 -TimeoutSec 10 }
$resp12 = if ($rl2.resp) { $rl2.resp | ConvertTo-Json -Compress -Depth 4 } else { $rl2.ed }
$auditDenied = if ($rl2.ok) { ($rl2.resp.success -eq $false) } else { ($rl2.status -eq 401) -or ($rl2.status -eq 403) }
Add-R 'nonadmin_audit_denied' $auditDenied "status=$($rl2.status) body=$resp12"

# 11. Owner (admin) can read own result/log -> success:true (positive control):
# proves the cross-user denial above is ownership-based, not a blanket block.
$rrAdm = Try-Req { Invoke-RestMethod -Uri "$MasterUrl/api/tasks/$isoTaskId/log" -Headers $adm -TimeoutSec 10 }
$respAdm = if ($rrAdm.resp) { $rrAdm.resp | ConvertTo-Json -Compress -Depth 4 } else { '' }
Add-R 'owner_log_allowed' (($rrAdm.status -eq 200) -and ($respAdm -match '"success":true')) "status=$($rrAdm.status) body=$respAdm"

$passed = ($results | Where-Object { $_.pass }).Count
$failed = ($results | Where-Object { -not $_.pass }).Count
Write-Output ""
Write-Output "SECURITY SUMMARY: $passed passed, $failed failed"
$results | ConvertTo-Json -Depth 4
if ($failed -gt 0) { exit 1 }

Write-Output "TEST_DONE_security-test"
