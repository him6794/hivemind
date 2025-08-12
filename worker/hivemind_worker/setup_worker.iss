; -- HiveMind Worker Inno Setup Script (Optimized English Version) --

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

[Files]
; License agreement (for display only, not installed)
Source: "license_agreement.txt"; Flags: dontcopy
; PowerShell script as standalone file
Source: "setup_logic.ps1"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Run]
; 1. Check and install Python 3.12
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Command python -ErrorAction SilentlyContinue)) {{ Start-Process -Wait -FilePath '{tmp}\python-installer.exe' -ArgumentList '/quiet InstallAllUsers=1 PrependPath=1' }} else {{ Write-Host 'Python already installed, skipping...' -ForegroundColor Green }}"""; \
    StatusMsg: "Checking Python 3.12..."; Flags: runhidden waituntilterminated; \
    BeforeInstall: "DownloadPython"

; 2. Check and install WireGuard
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Service -Name WireGuardManager -ErrorAction SilentlyContinue)) {{ Start-Process -Wait -FilePath '{tmp}\wireguard-installer.exe' -ArgumentList '/install /quiet' }} else {{ Write-Host 'WireGuard already installed, skipping...' -ForegroundColor Green }}"""; \
    StatusMsg: "Installing WireGuard..."; Flags: runhidden waituntilterminated; \
    BeforeInstall: "DownloadWireGuard"

; 3. Check and install Docker Desktop
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -Command ""if (-not (Get-Process 'Docker Desktop' -ErrorAction SilentlyContinue)) {{ Start-Process -Wait -FilePath '{tmp}\docker-desktop-installer.exe' }} else {{ Write-Host 'Docker Desktop already running, skipping...' -ForegroundColor Green }}"""; \
    StatusMsg: "Installing Docker Desktop..."; \
    BeforeInstall: "DownloadDocker"

; 4. Execute setup logic
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -File ""{tmp}\setup_logic.ps1"" -AppDir ""{app}"""; \
    StatusMsg: "Finalizing installation..."; Flags: runhidden waituntilterminated

; 5. Launch application
Filename: "{app}\start_hivemind.bat"; Description: "Launch HiveMind Worker"; Flags: nowait postinstall skipifsilent

[Code]
procedure DownloadPython;
begin
  if not FileExists(ExpandConstant('{tmp}\python-installer.exe')) then
  begin
    MsgBox('Downloading Python 3.12...', mbInformation, MB_OK);
    DownloadTemporaryFile('https://www.python.org/ftp/python/3.12.0/python-3.12.0-amd64.exe', 'python-installer.exe', '', nil);
  end;
end;

procedure DownloadWireGuard;
begin
  if not FileExists(ExpandConstant('{tmp}\wireguard-installer.exe')) then
  begin
    MsgBox('Downloading WireGuard...', mbInformation, MB_OK);
    DownloadTemporaryFile('https://download.wireguard.com/windows-client/wireguard-installer.exe', 'wireguard-installer.exe', '', nil);
  end;
end;

procedure DownloadDocker;
begin
  if not FileExists(ExpandConstant('{tmp}\docker-desktop-installer.exe')) then
  begin
    MsgBox('Downloading Docker Desktop...', mbInformation, MB_OK);
    DownloadTemporaryFile('https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe', 'docker-desktop-installer.exe', '', nil);
  end;
end;

[Icons]
Name: "{group}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"
Name: "{autodesktop}\HiveMind Worker"; Filename: "{app}\start_hivemind.bat"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create desktop shortcut"; GroupDescription: "Additional options:"; Flags: unchecked

[UninstallDelete]
Type: filesandordirs; Name: "{app}\venv"
Type: files; Name: "{app}\start_hivemind.bat"