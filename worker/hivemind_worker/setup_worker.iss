; -- HiveMind Worker 安裝包 --
[Setup]
AppName=HiveMind Worker
AppVersion=1.1
DefaultDirName={pf}\HiveMindWorker
DefaultGroupName=HiveMind Worker
OutputDir=.
OutputBaseFilename=HiveMindWorkerSetup
Compression=lzma
SolidCompression=yes
LicenseFile=用戶條款.txt

[Files]
; 用戶條款（僅供安裝程式顯示，不會安裝到目標機器）
Source: "用戶條款.txt"; Flags: dontcopy

[Run]
; 安裝 Python 3.12
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Invoke-WebRequest -UseBasicParsing -OutFile ""{tmp}\python-installer.exe"" https://www.python.org/ftp/python/3.12.0/python-3.12.0-amd64.exe; Start-Process -Wait -FilePath ""{tmp}\python-installer.exe"" -ArgumentList '/quiet InstallAllUsers=1 PrependPath=1'"""; StatusMsg: "正在安裝 Python 3.12..."; Flags: runhidden

; 創建虛擬環境
Filename: "python"; Parameters: "-m venv ""{app}\venv"""; StatusMsg: "正在創建 Python 虛擬環境..."; Flags: runhidden

; 在虛擬環境中安裝 hivemind_worker
Filename: "{app}\venv\Scripts\pip.exe"; Parameters: "install hivemind_worker"; StatusMsg: "正在安裝 hivemind_worker..."; Flags: runhidden

; 安裝 WireGuard（官方安裝程式）
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Invoke-WebRequest -UseBasicParsing -OutFile wireguard-installer.exe https://download.wireguard.com/windows-client/wireguard-installer.exe; Start-Process -Wait -FilePath .\wireguard-installer.exe -ArgumentList '/install /quiet'"""; StatusMsg: "正在安裝 WireGuard..."; Flags: runhidden

; Docker 安裝步驟 (分為下載和安裝兩個階段)
; 階段 1: 下載 Docker Desktop 安裝包
; 將下載的檔案存放到臨時目錄 {tmp}
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Invoke-WebRequest -UseBasicParsing -OutFile ""{tmp}\docker-desktop-installer.exe"" https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe"""; StatusMsg: "正在下載 Docker Desktop 安裝包..."; Flags: runhidden
; 階段 2: 安裝 Docker Desktop
; 從臨時目錄執行非靜默安裝，將會顯示 Docker 安裝介面
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""Start-Process -Wait -FilePath ""{tmp}\docker-desktop-installer.exe"""""; StatusMsg: "正在安裝 Docker Desktop (請手動操作)..."; 

; 創建啟動批次檔
Filename: "cmd.exe"; Parameters: "/c echo @echo off > ""{app}\start_hivemind.bat"" && echo cd /d ""{app}"" >> ""{app}\start_hivemind.bat"" && echo call venv\Scripts\activate >> ""{app}\start_hivemind.bat"" && echo python -c ""import hivemind_worker; hivemind_worker.main()"" >> ""{app}\start_hivemind.bat"""; StatusMsg: "正在創建啟動腳本..."; Flags: runhidden

; 啟動主程式
Filename: "{app}\start_hivemind.bat"; Description: "啟動 HiveMind Worker"; Flags: nowait postinstall skipifsilent

[Icons]
Name: "{group}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"
Name: "{autodesktop}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "在桌面創建捷徑"; GroupDescription: "附加選項:"; Flags: unchecked

[UninstallDelete]
Type: filesandordirs; Name: "{app}\venv"
Type: files; Name: "{app}\start_hivemind.bat"