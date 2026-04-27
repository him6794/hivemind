# HiveMind Benchmark 編譯腳本
# 編譯 C 基準測試為共享庫（.dll/.so）

$ErrorActionPreference = "Stop"

Write-Host "=" -NoNewline; Write-Host ("=" * 59)
Write-Host "HiveMind Benchmark Library Compiler"
Write-Host "=" -NoNewline; Write-Host ("=" * 59)
Write-Host ""

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$benchmarkDir = $scriptDir

Write-Host "[1] Checking tools..."

# 檢查 GCC
$hasGcc = $false
try {
    $gccVersion = & gcc --version 2>&1 | Select-Object -First 1
    Write-Host "    GCC: $gccVersion" -ForegroundColor Green
    $hasGcc = $true
} catch {
    Write-Host "    GCC: Not found" -ForegroundColor Yellow
}

# 檢查 MSVC
$hasMsvc = $false
try {
    $clVersion = & cl 2>&1 | Select-Object -First 1
    if ($clVersion -match "Microsoft") {
        Write-Host "    MSVC: Found" -ForegroundColor Green
        $hasMsvc = $true
    }
} catch {
    Write-Host "    MSVC: Not found" -ForegroundColor Yellow
}

Write-Host ""

# 編譯共享庫
Write-Host "[2] Compiling benchmark library..."
$source = Join-Path $benchmarkDir "benchmark.c"
$outputDll = Join-Path $benchmarkDir "benchmark.dll"
$outputExe = Join-Path $benchmarkDir "benchmark_test.exe"

if (-not (Test-Path $source)) {
    Write-Host "    Error: Source file not found: $source" -ForegroundColor Red
    exit 1
}

$compiled = $false

# 嘗試使用 GCC 編譯共享庫和測試程序
if ($hasGcc) {
    try {
        Write-Host "    Compiling shared library with GCC..." -ForegroundColor Cyan
        
        # 編譯 DLL
        $gccArgs = @("-O3", "-march=native", "-shared", "-o", $outputDll, $source)
        & gcc $gccArgs 2>&1 | Out-Null
        
        if ($LASTEXITCODE -eq 0 -and (Test-Path $outputDll)) {
            Write-Host "    ✓ Shared library compiled: benchmark.dll" -ForegroundColor Green
            
            # 編譯測試程序
            & gcc -O3 -march=native -DBUILD_STANDALONE -o $outputExe $source 2>&1 | Out-Null
            
            if ($LASTEXITCODE -eq 0 -and (Test-Path $outputExe)) {
                Write-Host "    ✓ Test executable compiled: benchmark_test.exe" -ForegroundColor Green
                $compiled = $true
            }
        }
    } catch {
        Write-Host "    GCC compilation failed: $_" -ForegroundColor Yellow
    }
}

# 如果 GCC 失敗，嘗試 MSVC
if (-not $compiled -and $hasMsvc) {
    try {
        Write-Host "    Compiling with MSVC..." -ForegroundColor Cyan
        
        # 編譯 DLL
        & cl /O2 /LD /Fe:$outputDll $source 2>&1 | Out-Null
        
        if ($LASTEXITCODE -eq 0 -and (Test-Path $outputDll)) {
            Write-Host "    ✓ Shared library compiled: benchmark.dll" -ForegroundColor Green
            
            # 編譯測試程序
            & cl /O2 /DBUILD_STANDALONE /Fe:$outputExe $source 2>&1 | Out-Null
            
            if ($LASTEXITCODE -eq 0 -and (Test-Path $outputExe)) {
                Write-Host "    ✓ Test executable compiled: benchmark_test.exe" -ForegroundColor Green
                $compiled = $true
            }
            
            # 清理 MSVC 產生的臨時文件
            Remove-Item (Join-Path $benchmarkDir "*.obj") -ErrorAction SilentlyContinue
            Remove-Item (Join-Path $benchmarkDir "*.exp") -ErrorAction SilentlyContinue
        }
    } catch {
        Write-Host "    MSVC compilation failed: $_" -ForegroundColor Yellow
    }
}

if (-not $compiled) {
    Write-Host "    ✗ Compilation failed" -ForegroundColor Red
    Write-Host "    Please install GCC (MinGW) or MSVC" -ForegroundColor Yellow
    exit 1
}

Write-Host ""

# 測試共享庫
Write-Host "[3] Testing benchmark library..."
if (Test-Path $outputExe) {
    Write-Host ""
    & $outputExe
    Write-Host ""
}

Write-Host "[4] Compilation Summary"
Write-Host "    Shared Library: " -NoNewline
if (Test-Path $outputDll) {
    Write-Host "✓ $outputDll" -ForegroundColor Green
} else {
    Write-Host "✗ Failed" -ForegroundColor Red
}

Write-Host "    Test Program: " -NoNewline
if (Test-Path $outputExe) {
    Write-Host "✓ $outputExe" -ForegroundColor Green
} else {
    Write-Host "✗ Failed" -ForegroundColor Red
}

Write-Host ""
Write-Host "Compilation complete! Python can now load benchmark.dll using ctypes." -ForegroundColor Green
