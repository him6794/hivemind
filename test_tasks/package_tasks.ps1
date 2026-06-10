#!/usr/bin/env pwsh

$ErrorActionPreference = "Stop"

$taskRoot = $PSScriptRoot
$tasks = @(
    "01_hello_world",
    "02_math_compute",
    "03_text_processing"
)

Write-Host "Packaging sample test tasks..." -ForegroundColor Cyan
Write-Host ""

$packaged = 0
$missing = @()

foreach ($task in $tasks) {
    $sourcePath = Join-Path -Path $taskRoot -ChildPath $task
    $zipPath = Join-Path -Path $taskRoot -ChildPath "$task.zip"

    if (Test-Path -LiteralPath $sourcePath -PathType Container) {
        Write-Host "Packaging $task..." -ForegroundColor Yellow
        $sourceItems = Get-ChildItem -LiteralPath $sourcePath -Force
        if ($sourceItems.Count -eq 0) {
            Write-Host "Source directory is empty: $sourcePath" -ForegroundColor Red
            $missing += $sourcePath
            continue
        }
        Compress-Archive -LiteralPath $sourceItems.FullName -DestinationPath $zipPath -Force
        Write-Host "Created $zipPath" -ForegroundColor Green
        $packaged += 1
    } else {
        Write-Host "Missing source directory: $sourcePath" -ForegroundColor Red
        $missing += $sourcePath
    }
}

Write-Host ""

if ($missing.Count -gt 0) {
    Write-Host "Sample task packaging incomplete. Missing $($missing.Count) source director$(if ($missing.Count -eq 1) { 'y' } else { 'ies' })." -ForegroundColor Red
    exit 1
}

Write-Host "Packaged $packaged sample task archive$(if ($packaged -eq 1) { '' } else { 's' })." -ForegroundColor Green
Write-Host "Upload the generated ZIP files through the Web UI or CLI." -ForegroundColor White
