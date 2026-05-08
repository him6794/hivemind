@echo off
REM ============================================================
REM HiveMind 全模組測試腳本
REM ============================================================
setlocal enabledelayedexpansion

set "SCRIPT_DIR=%~dp0"
cd /d "%SCRIPT_DIR%"

echo.
echo ============================================================
echo   HiveMind 全模組測試
echo   時間: %date% %time%
echo ============================================================
echo.

set TOTAL_TESTS=0
set PASSED_TESTS=0
set FAILED_TESTS=0

REM ============================================================
REM 1. Worker Service 測試
REM ============================================================
echo.
echo [1/5] 測試 Worker Service...
echo ============================================================
cd services\worker
go test -v ./pkg/executor > ..\..\test_worker_executor.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker Executor 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Worker Executor 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

go test -v ./internal/service > ..\..\test_worker_service.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker Service 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Worker Service 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

go test -v ./cmd/server > ..\..\test_worker_main.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Worker Main 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Worker Main 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

cd ..\..

REM ============================================================
REM 2. Nodepool Service 測試
REM ============================================================
echo.
echo [2/5] 測試 Nodepool Service...
echo ============================================================
cd services\nodepool

go test -v ./internal/repository > ..\..\test_nodepool_repository.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool Repository 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Nodepool Repository 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

go test -v ./internal/service > ..\..\test_nodepool_service.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool Service 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Nodepool Service 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

go test -v ./cmd/server > ..\..\test_nodepool_main.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Nodepool Main 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Nodepool Main 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

cd ..\..

REM ============================================================
REM 3. Master Service 測試
REM ============================================================
echo.
echo [3/5] 測試 Master Service...
echo ============================================================
cd services\master

go test -v ./internal/bt > ..\..\test_master_bt.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Master BT 測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Master BT 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

go test -v ./... > ..\..\test_master_all.log 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [PASS] Master 所有測試通過
    set /a PASSED_TESTS+=1
) else (
    echo [FAIL] Master 測試失敗
    set /a FAILED_TESTS+=1
)
set /a TOTAL_TESTS+=1

cd ..\..

REM ============================================================
REM 4. Task 模組測試
REM ============================================================
echo.
echo [4/5] 測試 Task 模組...
echo ============================================================

REM 檢查 Python 是否可用
python --version >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    cd task\tests
    python -m pytest test_api_queue_unittest.py -v > ..\..\test_task_unittest.log 2>&1
    if %ERRORLEVEL% EQU 0 (
        echo [PASS] Task API Queue 測試通過
        set /a PASSED_TESTS+=1
    ) else (
        echo [FAIL] Task API Queue 測試失敗
        set /a FAILED_TESTS+=1
    )
    set /a TOTAL_TESTS+=1
    cd ..\..
) else (
    echo [SKIP] Python 不可用，跳過 Task 測試
)

REM ============================================================
REM 5. 整合測試
REM ============================================================
echo.
echo [5/5] 整合測試...
echo ============================================================

REM 檢查 test_download_extract_execute.py
if exist test_download_extract_execute.py (
    python test_download_extract_execute.py > test_integration.log 2>&1
    if %ERRORLEVEL% EQU 0 (
        echo [PASS] 下載解壓執行測試通過
        set /a PASSED_TESTS+=1
    ) else (
        echo [FAIL] 下載解壓執行測試失敗
        set /a FAILED_TESTS+=1
    )
    set /a TOTAL_TESTS+=1
) else (
    echo [SKIP] test_download_extract_execute.py 不存在
)

REM ============================================================
REM 測試總結
REM ============================================================
echo.
echo ============================================================
echo   測試總結
echo ============================================================
echo   總測試數: %TOTAL_TESTS%
echo   通過: %PASSED_TESTS%
echo   失敗: %FAILED_TESTS%
echo ============================================================
echo.

if %FAILED_TESTS% EQU 0 (
    echo [SUCCESS] 所有測試通過！
    echo.
    echo 詳細日誌文件:
    echo   - test_worker_executor.log
    echo   - test_worker_service.log
    echo   - test_worker_main.log
    echo   - test_nodepool_repository.log
    echo   - test_nodepool_service.log
    echo   - test_nodepool_main.log
    echo   - test_master_bt.log
    echo   - test_master_all.log
    echo   - test_task_unittest.log
    echo   - test_integration.log
    exit /b 0
) else (
    echo [FAILURE] 有 %FAILED_TESTS% 個測試失敗
    echo.
    echo 請檢查日誌文件以獲取詳細信息
    exit /b 1
)
