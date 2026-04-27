# 編譯 prime_sieve.c 成 .so/.dll
# Windows 需要安裝 MinGW-w64 或 Visual Studio

$ErrorActionPreference = "Stop"

Write-Host "開始編譯 prime_sieve.c..." -ForegroundColor Green

# 檢查是否有 gcc
$gcc = Get-Command gcc -ErrorAction SilentlyContinue
if ($gcc) {
    Write-Host "使用 GCC 編譯（啟用 OpenMP 多執行緒）..." -ForegroundColor Yellow
    # 編譯成 .so（啟用 OpenMP）
    gcc -shared -O3 -march=native -fPIC -fopenmp -o prime_sieve.so prime_sieve.c -lm
    if ($LASTEXITCODE -eq 0) {
        Write-Host "✓ 編譯成功: prime_sieve.so (with OpenMP)" -ForegroundColor Green
        Write-Host "檔案大小: $((Get-Item prime_sieve.so).Length) bytes"
    } else {
        Write-Host "✗ 編譯失敗" -ForegroundColor Red
        exit 1
    }
} else {
    # 嘗試用 cl (MSVC)
    $cl = Get-Command cl -ErrorAction SilentlyContinue
    if ($cl) {
        Write-Host "使用 MSVC 編譯..." -ForegroundColor Yellow
        cl /O2 /LD prime_sieve.c /link /OUT:prime_sieve.dll
        if ($LASTEXITCODE -eq 0) {
            Write-Host "✓ 編譯成功: prime_sieve.dll" -ForegroundColor Green
        } else {
            Write-Host "✗ 編譯失敗" -ForegroundColor Red
            exit 1
        }
    } else {
        Write-Host "✗ 找不到 gcc 或 cl 編譯器" -ForegroundColor Red
        Write-Host "請安裝 MinGW-w64 或 Visual Studio" -ForegroundColor Yellow
        Write-Host "MinGW-w64: https://www.mingw-w64.org/" -ForegroundColor Cyan
        exit 1
    }
}

Write-Host "`n測試編譯結果..." -ForegroundColor Yellow
python -c "import ctypes; lib = ctypes.CDLL('./prime_sieve.so'); print(f'count_primes(1, 100) = {lib.count_primes(1, 100)}')"
