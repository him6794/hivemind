#!/usr/bin/env pwsh
# 打包所有測試任務

Write-Host "正在打包測試任務..." -ForegroundColor Cyan
Write-Host ""

$tasks = @(
    "01_hello_world",
    "02_math_compute",
    "03_text_processing"
)

foreach ($task in $tasks) {
    $sourcePath = "test_tasks/$task"
    $zipPath = "test_tasks/$task.zip"

    if (Test-Path $sourcePath) {
        Write-Host "打包 $task..." -ForegroundColor Yellow
        Compress-Archive -Path "$sourcePath/*" -DestinationPath $zipPath -Force
        Write-Host "✓ 已建立: $zipPath" -ForegroundColor Green
    } else {
        Write-Host "✗ 找不到: $sourcePath" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "所有測試任務已打包完成！" -ForegroundColor Green
Write-Host ""
Write-Host "可用的測試任務:" -ForegroundColor Yellow
Write-Host "  1. 01_hello_world.zip      - 簡單的 Hello World 任務" -ForegroundColor White
Write-Host "  2. 02_math_compute.zip     - 數學計算任務（質數、費波那契）" -ForegroundColor White
Write-Host "  3. 03_text_processing.zip  - 文字處理任務" -ForegroundColor White
Write-Host ""
Write-Host "使用方式:" -ForegroundColor Yellow
Write-Host "  在 Web UI 中上傳這些 ZIP 檔案即可測試" -ForegroundColor White
Write-Host ""
