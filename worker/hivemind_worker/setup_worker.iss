; -- HiveMind Worker Inno Setup Script (Final Correct Version) --

[Setup]
AppName=HiveMind Worker
AppVersion=2.0
PrivilegesRequired=admin
UsePreviousAppDir=no
DefaultDirName=C:\HiveMindWorker
DefaultGroupName=HiveMind Worker
OutputDir=.
OutputBaseFilename=HiveMindWorkerSetup
Compression=lzma
SolidCompression=yes
LicenseFile=用戶條款.txt

[Files]
; 用戶條款（僅供安裝程式顯示，不會安裝到目標機器）
Source: "用戶條款.txt"; Flags: dontcopy
; {cm:Fix_Final} 將 PowerShell 腳本作為一個獨立檔案包含進來
Source: "setup_logic.ps1"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Run]
; 1. 檢查並安裝 Python 3.12
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Command python -ErrorAction SilentlyContinue)) {{ Write-Host '正在下載 Python 3.12...'; Invoke-WebRequest -UseBasicParsing -OutFile '{tmp}\python-installer.exe' 'https://www.python.org/ftp/python/3.12.0/python-3.12.0-amd64.exe'; Write-Host '正在安裝 Python 3.12...'; Start-Process -Wait -FilePath '{tmp}\python-installer.exe' -ArgumentList '/quiet InstallAllUsers=1 PrependPath=1'; Write-Host '重新載入環境變數...'; $env:Path = [System.Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [System.Environment]::GetEnvironmentVariable('Path','User') }} else {{ Write-Host 'Python 已安裝，跳過安裝步驟' }}"""; StatusMsg: "檢查並安裝 Python 3.12..."; Flags: runhidden waituntilterminated

; 2. 檢查並安裝 WireGuard
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Service -Name WireGuardManager -ErrorAction SilentlyContinue)) {{ Write-Host '正在下載 WireGuard...'; Invoke-WebRequest -UseBasicParsing -OutFile '{tmp}\wireguard-installer.exe' 'https://download.wireguard.com/windows-client/wireguard-installer.exe'; Write-Host '正在安裝 WireGuard...'; Start-Process -Wait -FilePath '{tmp}\wireguard-installer.exe' -ArgumentList '/install /quiet' }} else {{ Write-Host 'WireGuard 已安裝，跳過安裝步驟' }}"""; StatusMsg: "檢查並安裝 WireGuard..."; Flags: runhidden waituntilterminated

; 3. 檢查並安裝 Docker Desktop
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Process 'Docker Desktop' -ErrorAction SilentlyContinue)) {{ if (-not (Test-Path '{tmp}\docker-desktop-installer.exe')) {{ Write-Host '正在下載 Docker Desktop...'; Invoke-WebRequest -UseBasicParsing -OutFile '{tmp}\docker-desktop-installer.exe' 'https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe' }}; Write-Host '正在啟動 Docker Desktop 安裝程式...'; Start-Process -FilePath '{tmp}\docker-desktop-installer.exe' }} else {{ Write-Host 'Docker Desktop 已在運行，跳過安裝步驟' }}"""; StatusMsg: "檢查並安裝 Docker Desktop...";

; {cm:Fix_Final} 執行外部的 PowerShell 腳本，並將安裝路徑 {app} 作為參數傳遞給它
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{tmp}\setup_logic.ps1"" -AppDir ""{app}"""; StatusMsg: "創建虛擬環境、安裝套件並建立啟動檔..."; Flags: runhidden waituntilterminated

; 4. 啟動主程式
Filename: "{app}\start_hivemind.bat"; Description: "啟動 HiveMind Worker"; Flags: nowait postinstall skipifsilent

[Icons]
Name: "{group}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"
Name: "{autodesktop}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "在桌面創建捷徑"; GroupDescription: "附加選項:"; Flags: unchecked

[UninstallDelete]
Type: filesandordirs; Name: "{app}\venv"
Type: files; Name: "{app}\start_hivemind.bat"