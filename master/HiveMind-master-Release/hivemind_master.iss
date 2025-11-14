[Setup]
AppName=HiveMind master
AppVersion=2.0
DefaultDirName=C:\HiveMindmaster
DefaultGroupName=HiveMind master
OutputBaseFilename=HiveMindmasterSetup
Compression=lzma
SolidCompression=yes
PrivilegesRequired=admin
LicenseFile=用戶條款.txt
DisableDirPage=yes
DisableProgramGroupPage=yes

[Languages]

[Files]
; 你必須準備這三個安裝包，放在 .iss 同資料夾
Source: "python-3.12.0-amd64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall
Source: "WireGuard-installer.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

; 啟動檔案
Source: "start_hivemind.cmd"; DestDir: "{app}"

; 用戶條款
Source: "用戶條款.txt"; DestDir: "{app}"

[Icons]
Name: "{autodesktop}\HiveMind master"; Filename: "{app}\start_hivemind.cmd"; WorkingDir: "{app}"

[Run]
; 安裝 Python
Filename: "{tmp}\python-3.12.0-amd64.exe"; Parameters: "/quiet InstallAllUsers=1 PrependPath=1"; Flags: waituntilterminated

; 安裝 WireGuard
Filename: "{tmp}\WireGuard-installer.exe"; Parameters: "/S"; Flags: waituntilterminated

; 建立虛擬環境
Filename: "python.exe"; Parameters: "-m venv ""{app}\venv"""; Flags: waituntilterminated
