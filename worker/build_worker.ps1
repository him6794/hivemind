# HiveMind Worker Node - Nuitka Build Script
# 此腳本會自動編譯 worker_node.py 並複製必要的 DLL 檔案

param(
    [switch]$Debug,  # 使用 --windows-console-mode=attach 來顯示主控台（用於除錯）
    [switch]$Clean   # 清除舊的編譯結果
)

$ErrorActionPreference = "Stop"

# 設定路徑
$ProjectRoot = "C:\Users\user\Desktop\hivemind"
$WorkerSrc = "$ProjectRoot\worker\src\hivemind_worker"
$OutputDir = "$ProjectRoot\dist"
$PythonExe = "C:\Users\user\AppData\Local\Programs\Python\Python312\python.exe"

Write-Host "=== HiveMind Worker Node Build Script ===" -ForegroundColor Cyan
Write-Host "Project Root: $ProjectRoot"
Write-Host "Output Directory: $OutputDir"

# 確保 protobuf 檔案存在
Write-Host "`n[1/4] Checking protobuf files..." -ForegroundColor Yellow
$pb2Files = @("nodepool_pb2.py", "nodepool_pb2_grpc.py")
foreach ($file in $pb2Files) {
    $srcFile = "$ProjectRoot\node_pool\$file"
    $dstFile = "$WorkerSrc\$file"
    
    if (Test-Path $srcFile) {
        Copy-Item $srcFile $dstFile -Force
        Write-Host "  ✓ Copied $file" -ForegroundColor Green
    } else {
        Write-Host "  ✗ Missing $file - run protoc task first!" -ForegroundColor Red
        exit 1
    }
}

# 嘗試（可選）編譯 Go 指標 DLL：psutil.dll
Write-Host "`n[0/4] Optional: Building Go metrics DLL (psutil.dll)..." -ForegroundColor Yellow
try {
    $goFile = Join-Path $WorkerSrc 'psutil.go'
    $dllFile = Join-Path $WorkerSrc 'psutil.dll'
    if (Test-Path $goFile) {
        $needBuild = $true
        if (Test-Path $dllFile) {
            $dllTime = (Get-Item $dllFile).LastWriteTime
            $goTime = (Get-Item $goFile).LastWriteTime
            if ($dllTime -ge $goTime) { $needBuild = $false }
        }
        if ($needBuild) {
            # 需要 C 編譯器 (gcc) 與 CGO
            $gcc = Get-Command gcc -ErrorAction SilentlyContinue
            if (-not $gcc) {
                Write-Host "  ⚠ gcc not found. Skipping DLL build. Install MSYS2 MinGW-w64 or TDM-GCC to enable it." -ForegroundColor Yellow
                Write-Host "    For MSYS2 (recommended): winget install -e --id MSYS2.MSYS2" -ForegroundColor DarkGray
                Write-Host "    Then in MSYS2: pacman -S --needed base-devel mingw-w64-x86_64-toolchain" -ForegroundColor DarkGray
                Write-Host "    Add C:\msys64\mingw64\bin to PATH, then re-run build." -ForegroundColor DarkGray
            }
            else {
                $env:CGO_ENABLED = "1"
                Push-Location $WorkerSrc
                Write-Host "  > go build -o psutil.dll -buildmode=c-shared psutil.go" -ForegroundColor Gray
                & go build -o psutil.dll -buildmode=c-shared psutil.go
                Pop-Location
                if (Test-Path $dllFile) {
                    Write-Host "  ✓ psutil.dll built" -ForegroundColor Green
                } else {
                    Write-Host "  ⚠ go build finished but psutil.dll not found (see console for errors)." -ForegroundColor Yellow
                }
            }
        }
        else {
            Write-Host "  ✓ psutil.dll is up-to-date" -ForegroundColor Green
        }
    } else {
        Write-Host "  (skip) psutil.go not present" -ForegroundColor DarkGray
    }
}
catch {
    Write-Host "  ⚠ Failed to build psutil.dll: $($_.Exception.Message)" -ForegroundColor Yellow
}

# 編譯選項
Write-Host "`n[2/4] Compiling with Nuitka..." -ForegroundColor Yellow

# 清理輸出目錄以避免 SameFileError
$DistDir = "$OutputDir\worker_node.dist"
if (Test-Path $DistDir) {
    Write-Host "  Cleaning existing output directory..." -ForegroundColor Gray
    
    # 確保不在要刪除的目錄中
    $CurrentLocation = Get-Location
    if ($CurrentLocation.Path.StartsWith($DistDir)) {
        Write-Host "  Switching out of output directory..." -ForegroundColor Gray
        Set-Location $ProjectRoot
    }
    
    try {
        # 使用更強力的刪除方法
        Remove-Item -Path $DistDir -Recurse -Force -ErrorAction Stop
        Write-Host "  ✓ Old build directory removed" -ForegroundColor Green
        
        # 確保完全刪除（有時需要短暫等待）
        Start-Sleep -Milliseconds 500
        
        if (Test-Path $DistDir) {
            Write-Host "  ⚠ Directory still exists, using robocopy to force clean..." -ForegroundColor Yellow
            $EmptyDir = "$env:TEMP\nuitka_empty_$(Get-Random)"
            New-Item -ItemType Directory -Path $EmptyDir -Force | Out-Null
            & robocopy $EmptyDir $DistDir /MIR /NJH /NJS /NFL /NDL | Out-Null
            Remove-Item -Path $DistDir -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -Path $EmptyDir -Force -ErrorAction SilentlyContinue
            Write-Host "  ✓ Force cleaned with robocopy" -ForegroundColor Green
        }
    }
    catch {
        Write-Host "  ⚠ Failed to remove old directory: $($_.Exception.Message)" -ForegroundColor Yellow
        Write-Host "  Attempting alternative cleanup method..." -ForegroundColor Gray
        
        # 備用方法：重命名後刪除
        try {
            $BackupDir = "$OutputDir\worker_node.dist.old_$(Get-Random)"
            Rename-Item -Path $DistDir -NewName $BackupDir -Force
            Remove-Item -Path $BackupDir -Recurse -Force -ErrorAction SilentlyContinue
            Write-Host "  ✓ Old directory renamed and scheduled for deletion" -ForegroundColor Green
        }
        catch {
            Write-Host "  ✗ Could not clean directory. Please manually delete '$DistDir' and retry." -ForegroundColor Red
            exit 1
        }
    }
}

$ConsoleMode = if ($Debug) { "attach" } else { "disable" }

$NuitkaArgs = @(
    "-m", "nuitka",
    "--standalone",
    "--windows-console-mode=$ConsoleMode",
    "--follow-imports",
    "--include-data-dir=`"$WorkerSrc\templates=hivemind_worker\templates`"",
    "--include-data-dir=`"$WorkerSrc\static=hivemind_worker\static`"",
    "--output-dir=`"$OutputDir`"",
    "--remove-output"  # 強制清理舊輸出以避免衝突
)

$NuitkaArgs += "`"$WorkerSrc\worker_node.py`""

Write-Host "  Console Mode: $ConsoleMode" -ForegroundColor Gray
Write-Host "  Output will be cleaned before compilation" -ForegroundColor Gray

& $PythonExe $NuitkaArgs

if ($LASTEXITCODE -ne 0) {
    Write-Host "`n✗ Compilation failed!" -ForegroundColor Red
    exit $LASTEXITCODE
}

Write-Host "`n✓ Compilation successful!" -ForegroundColor Green

# 複製 DLL 檔案和其他必要檔案
Write-Host "`n[3/4] Copying DLL files and dependencies..." -ForegroundColor Yellow
# DistDir 已在前面定義，這裡直接使用
# $DistDir = "$OutputDir\worker_node.dist"

# 複製 DLL 檔案
$DllFiles = @("psutil.dll", "wireguardlib.dll", "wintun.dll")
foreach ($dll in $DllFiles) {
    $srcDll = "$WorkerSrc\$dll"
    if (Test-Path $srcDll) {
        Copy-Item $srcDll $DistDir -Force
        Write-Host "  ✓ Copied $dll" -ForegroundColor Green
    } else {
        Write-Host "  ⚠ $dll not found (optional)" -ForegroundColor Yellow
    }
}

# 複製配置檔案 worker_credentials.json
Write-Host "`n[3.1] Copying configuration files..." -ForegroundColor Yellow
$ConfigFile = "$WorkerSrc\worker_credentials.json"
$TargetConfigDir = "$DistDir\hivemind_worker"
if (Test-Path $ConfigFile) {
    # 確保目標目錄存在
    if (-not (Test-Path $TargetConfigDir)) {
        New-Item -ItemType Directory -Force -Path $TargetConfigDir | Out-Null
    }

    # 主要：放到 dist 根目錄（與 worker_node.exe 同層）
    Copy-Item $ConfigFile "$DistDir\worker_credentials.json" -Force
    Write-Host "  ✓ Copied worker_credentials.json to dist root" -ForegroundColor Green

    # 相容：保留舊路徑 hivemind_worker\worker_credentials.json
    Copy-Item $ConfigFile "$TargetConfigDir\worker_credentials.json" -Force
    Write-Host "  ✓ Copied worker_credentials.json to hivemind_worker\" -ForegroundColor Green
} else {
    Write-Host "  ✗ worker_credentials.json not found! Worker will not work without it." -ForegroundColor Red
    Write-Host "    Please create $ConfigFile with nodepool_address and node_port" -ForegroundColor Red
}

# 複製 VPN 相關檔案（打包用）
Write-Host "`n[3.15] Copying VPN assets (for launcher)..." -ForegroundColor Yellow
$VpnSrcDir = "$ProjectRoot\vpn"
$VpnDstDir = "$DistDir\vpn"
if (-not (Test-Path $VpnDstDir)) {
    New-Item -ItemType Directory -Force -Path $VpnDstDir | Out-Null
}

$VpnFiles = @(
    "hivemind-vpn.exe",
    "wg0.conf",
    "wireguard.conf",
    "wintun.dll",
    "start_vpn.ps1",
    "start_vpn.bat"
)

foreach ($vf in $VpnFiles) {
    $src = Join-Path $VpnSrcDir $vf
    if (Test-Path $src) {
        Copy-Item $src $VpnDstDir -Force
        Write-Host "  ✓ Copied vpn\\$vf" -ForegroundColor Green

        # 同級目錄也放一份，方便 launcher 直接呼叫：hivemind-vpn.exe wg0.conf --auto-retry
        if ($vf -in @("hivemind-vpn.exe", "wg0.conf", "wireguard.conf", "wintun.dll")) {
            Copy-Item $src $DistDir -Force
            Write-Host "  ✓ Copied $vf to dist root" -ForegroundColor Green
        }
    } else {
        Write-Host "  ⚠ vpn\\$vf not found (optional)" -ForegroundColor Yellow
    }
}

# worker 的 VPN loader 會找 vpn\\wireguardlib.dll / vpn\\wintun.dll
if (Test-Path "$DistDir\wireguardlib.dll") {
    Copy-Item "$DistDir\wireguardlib.dll" "$VpnDstDir\wireguardlib.dll" -Force
}
if (Test-Path "$DistDir\wintun.dll") {
    Copy-Item "$DistDir\wintun.dll" "$VpnDstDir\wintun.dll" -Force
}

# 嘗試自動複製 pywebview 套件與其 native 檔案（若已安裝）
Write-Host "`n[3.2] Copy pywebview package and native libs (if present)" -ForegroundColor Yellow
try {
    $pyCmd = @"
import webview, os, shutil
pkg = os.path.dirname(webview.__file__)
print(pkg)
files = []
for root,dirs,fnames in os.walk(pkg):
    for f in fnames:
        if f.lower().endswith(('.dll','.pyd','.so','.dll.config','webview.dll','WebView2Loader.dll')):
            files.append(os.path.join(root,f))
print('\n'.join(files))
"@

    $out = & $PythonExe -c $pyCmd 2>$null
    if ($LASTEXITCODE -eq 0 -and $out) {
        $lines = $out -split "`n"
        $pkgPath = $lines[0].Trim()
        if (Test-Path $pkgPath) {
            $destPkg = Join-Path $DistDir (Split-Path $pkgPath -Leaf)
            Write-Host "  Copying pywebview package from: $pkgPath -> $destPkg" -ForegroundColor Gray
            Copy-Item $pkgPath $destPkg -Recurse -Force
            # Copy listed native files (if any)
            foreach ($i in 1..($lines.Length - 1)) {
                $f = $lines[$i].Trim()
                if ($f) {
                    try {
                        Copy-Item $f $DistDir -Force
                        Write-Host "    ✓ Copied native: $(Split-Path $f -Leaf)" -ForegroundColor Green
                    } catch {
                        Write-Host "    ⚠ Failed to copy native: $($f) - $($_.Exception.Message)" -ForegroundColor Yellow
                    }
                }
            }
        } else {
            Write-Host "  ⚠ pywebview package path not found: $pkgPath" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  ⚠ pywebview not found in Python environment or error occurred." -ForegroundColor Yellow
    }
} catch {
    Write-Host "  ⚠ Error while copying pywebview package: $($_.Exception.Message)" -ForegroundColor Yellow
}

# （可選）編譯 Go 啟動器到 dist 目錄
Write-Host "`n[3.25] Building Go launcher (optional)..." -ForegroundColor Yellow
try {
    $goCmd = Get-Command go -ErrorAction SilentlyContinue
    if ($goCmd) {
        $LauncherSrc = "$ProjectRoot\worker_launcher"
        if (Test-Path "$LauncherSrc\main.go") {
            Push-Location $LauncherSrc
            $env:CGO_ENABLED = "0"
            & go build -o "$DistDir\hivemind-launcher.exe" .
            Pop-Location
            if (Test-Path "$DistDir\hivemind-launcher.exe") {
                Write-Host "  ✓ Built hivemind-launcher.exe" -ForegroundColor Green
            } else {
                Write-Host "  ⚠ go build finished but launcher not found" -ForegroundColor Yellow
            }
        } else {
            Write-Host "  ⚠ launcher source not found at $LauncherSrc" -ForegroundColor Yellow
        }
    } else {
        Write-Host "  (skip) go not found in PATH" -ForegroundColor DarkGray
    }
}
catch {
    Write-Host "  ⚠ Failed to build launcher: $($_.Exception.Message)" -ForegroundColor Yellow
}

# 複製 Go 檔案（如果需要編譯 psutil.dll）
$GoFiles = @("psutil.go", "psutil.h", "go.mod")
foreach ($goFile in $GoFiles) {
    $srcGo = "$WorkerSrc\$goFile"
    if (Test-Path $srcGo) {
        Copy-Item $srcGo $DistDir -Force
        Write-Host "  ✓ Copied $goFile (optional)" -ForegroundColor Gray
    }
}

# 複製內建 Python Runtime（B1 方案，可選）
Write-Host "`n[3.2] Copy bundled Python runtime (optional, B1)" -ForegroundColor Yellow
try {
    $BundledPySrc = "$ProjectRoot\worker\runtime\python"
    $BundledPyDst = "$DistDir\runtime\python"
    if (Test-Path $BundledPySrc) {
        Write-Host "  Copying bundled python: $BundledPySrc -> $BundledPyDst" -ForegroundColor Gray
        New-Item -ItemType Directory -Force -Path $BundledPyDst | Out-Null
        Copy-Item "$BundledPySrc\*" $BundledPyDst -Recurse -Force
        Write-Host "  ✓ Bundled Python runtime copied" -ForegroundColor Green

        # 確保 pip 可用（embeddable python 常見缺 pip）
        Write-Host "`n[3.3] Ensure pip in bundled Python (optional, B1)" -ForegroundColor Yellow
        $BundledPyExe = "$BundledPyDst\python.exe"
        $Bootstrapper = "$ProjectRoot\worker\tools\bootstrap_pip.py"
        if (Test-Path $BundledPyExe -and Test-Path $Bootstrapper) {
            try {
                # 先嘗試檢查 pip
                $pipCheck = & $BundledPyExe -m pip --version 2>$null
                if ($LASTEXITCODE -ne 0) {
                    Write-Host "  pip not found, bootstrapping..." -ForegroundColor Yellow
                    & $PythonExe $Bootstrapper --python $BundledPyExe
                    if ($LASTEXITCODE -eq 0) {
                        Write-Host "  ✓ pip bootstrap succeeded" -ForegroundColor Green
                    } else {
                        Write-Host "  ⚠ pip bootstrap failed (worker can still run, but venv tasks may fail offline)." -ForegroundColor Yellow
                    }
                } else {
                    Write-Host "  ✓ pip already present in bundled python" -ForegroundColor Green
                }
            }
            catch {
                Write-Host "  ⚠ Error while ensuring pip: $($_.Exception.Message)" -ForegroundColor Yellow
            }
        } else {
            Write-Host "  (skip) bundled python.exe or bootstrap script not found." -ForegroundColor DarkGray
        }
    } else {
        Write-Host "  (skip) $BundledPySrc not found. Create it to enable no-system-python task fallback." -ForegroundColor DarkGray
    }
}
catch {
    Write-Host "  ⚠ Failed to copy bundled Python runtime: $($_.Exception.Message)" -ForegroundColor Yellow
}

# 顯示結果
Write-Host "`n[4/4] Build Summary" -ForegroundColor Yellow
Write-Host "  Executable: $DistDir\worker_node.exe" -ForegroundColor Cyan
Write-Host "  Console Mode: $ConsoleMode" -ForegroundColor Cyan

$ExeSize = (Get-Item "$DistDir\worker_node.exe").Length / 1MB
Write-Host "  Executable Size: $([math]::Round($ExeSize, 2)) MB" -ForegroundColor Cyan

Write-Host "`n=== Build Complete! ===" -ForegroundColor Green
Write-Host "`nTo run the worker:"
Write-Host "  cd $DistDir"
Write-Host "  .\worker_node.exe`n"

# 同步發佈目錄：dist\worker_node（launcher 預設使用這個路徑）
Write-Host "`n[4.5] Syncing release folder (dist\\worker_node)..." -ForegroundColor Yellow
$ReleaseDir = "$OutputDir\worker_node"
if (-not (Test-Path $ReleaseDir)) {
    New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null
}

try {
    # 使用 robocopy 做鏡像同步（更耐用，避免 Copy-Item 在大量檔案時不穩）
    & robocopy $DistDir $ReleaseDir /MIR /R:2 /W:1 /NJH /NJS /NFL /NDL | Out-Null
    $rc = $LASTEXITCODE
    if ($rc -ge 8) {
        Write-Host "  ✗ Failed to sync release folder (robocopy exit code: $rc)" -ForegroundColor Red
        exit 1
    }
    # robocopy 的成功碼不一定是 0，避免污染後續流程
    $global:LASTEXITCODE = 0
    Write-Host "  ✓ Release folder synced: $ReleaseDir" -ForegroundColor Green
}
catch {
    Write-Host "  ✗ Failed to sync release folder: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
