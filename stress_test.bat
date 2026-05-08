@echo off
REM ============================================================
REM HiveMind 壓力測試腳本
REM ============================================================
setlocal enabledelayedexpansion

set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%"

echo.
echo ============================================================
echo   HiveMind 壓力測試
echo   時間: %date% %time%
echo ============================================================
echo.

REM ============================================================
REM 1. Go 測試壓力測試 (使用 -count 參數重複執行)
REM ============================================================
echo.
echo [1/4] Go 模組壓力測試 (重複執行 10 次)...
echo ============================================================

echo.
echo [1.1] Worker Executor 壓力測試...
cd services\worker
go test -v -count=10 ./pkg/executor > ..\..\stress_worker_executor.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker Executor 壓力測試通過
) else (
    echo [FAIL] Worker Executor 壓力測試失敗
)
cd ..\..

echo.
echo [1.2] Nodepool 壓力測試...
cd services\nodepool
go test -v -count=10 ./cmd/server > ..\..\stress_nodepool.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool 壓力測試通過
) else (
    echo [FAIL] Nodepool 壓力測試失敗
)
cd ..\..

REM ============================================================
REM 2. 並發測試 (使用 -parallel 參數)
REM ============================================================
echo.
echo [2/4] 並發測試 (8 個並發)...
echo ============================================================

echo.
echo [2.1] Worker 並發測試...
cd services\worker
go test -v -parallel=8 ./... > ..\..\stress_worker_parallel.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker 並發測試通過
) else (
    echo [FAIL] Worker 並發測試失敗
)
cd ..\..

echo.
echo [2.2] Nodepool 並發測試...
cd services\nodepool
go test -v -parallel=8 ./... > ..\..\stress_nodepool_parallel.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool 並發測試通過
) else (
    echo [FAIL] Nodepool 並發測試失敗
)
cd ..\..

REM ============================================================
REM 3. 競態條件檢測 (使用 -race 參數)
REM ============================================================
echo.
echo [3/4] 競態條件檢測...
echo ============================================================

echo.
echo [3.1] Worker 競態檢測...
cd services\worker
go test -race -v ./... > ..\..\stress_worker_race.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker 無競態條件
) else (
    echo [FAIL] Worker 發現競態條件
)
cd ..\..

echo.
echo [3.2] Nodepool 競態檢測...
cd services\nodepool
go test -race -v ./... > ..\..\stress_nodepool_race.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool 無競態條件
) else (
    echo [FAIL] Nodepool 發現競態條件
)
cd ..\..

echo.
echo [3.3] Master 競態檢測...
cd services\master
go test -race -v ./... > ..\..\stress_master_race.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Master 無競態條件
) else (
    echo [FAIL] Master 發現競態條件
)
cd ..\..

REM ============================================================
REM 4. 基準測試 (使用 -bench 參數)
REM ============================================================
echo.
echo [4/4] 性能基準測試...
echo ============================================================

echo.
echo [4.1] Worker 基準測試...
cd services\worker
go test -bench=. -benchmem -benchtime=5s ./... > ..\..\stress_worker_bench.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker 基準測試完成
) else (
    echo [INFO] Worker 無基準測試或測試失敗
)
cd ..\..

echo.
echo [4.2] Nodepool 基準測試...
cd services\nodepool
go test -bench=. -benchmem -benchtime=5s ./... > ..\..\stress_nodepool_bench.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool 基準測試完成
) else (
    echo [INFO] Nodepool 無基準測試或測試失敗
)
cd ..\..

REM ============================================================
REM 測試總結
REM ============================================================
echo.
echo ============================================================
echo   壓力測試完成
echo ============================================================
echo.
echo 測試報告已生成:
echo   - stress_worker_executor.log    (重複執行 10 次)
echo   - stress_nodepool.log           (重複執行 10 次)
echo   - stress_worker_parallel.log    (8 個並發)
echo   - stress_nodepool_parallel.log  (8 個並發)
echo   - stress_worker_race.log        (競態檢測)
echo   - stress_nodepool_race.log      (競態檢測)
echo   - stress_master_race.log        (競態檢測)
echo   - stress_worker_bench.log       (性能基準)
echo   - stress_nodepool_bench.log     (性能基準)
echo.
echo 請檢查日誌文件以獲取詳細信息
echo.

pause
