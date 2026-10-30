; Per-user installer for a Hivemind Windows package directory (Master or Worker).
; Built by scripts/installer/build-windows-installers.ps1, which passes these defines:
;   APP_NAME, APP_ID, APP_VERSION, APP_VERSION_4, APP_EXE, APP_HELPER_EXE,
;   OUT_FILE, ICON_FILE, FILES_NSH (generated File directives), UNINSTALL_NSH (generated Delete directives).
; Installs to %LOCALAPPDATA%\Programs\Hivemind\<APP_ID>, so the app can write its own .env and sandbox
; without administrator rights. Uninstall removes the package files only; user files are kept.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ShowInstDetails show
ShowUninstDetails show

!ifndef APP_NAME
  !error "APP_NAME must be defined"
!endif
!ifndef APP_VERSION_4
  !error "APP_VERSION_4 must be defined"
!endif
!ifndef FILES_NSH
  !error "FILES_NSH must be defined"
!endif
!ifndef UNINSTALL_NSH
  !error "UNINSTALL_NSH must be defined"
!endif

!include "MUI2.nsh"
!include "LogicLib.nsh"

Name "${APP_NAME} ${APP_VERSION}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\Hivemind\${APP_ID}"
InstallDirRegKey HKCU "Software\Hivemind\${APP_ID}" "InstallLocation"

VIProductVersion "${APP_VERSION_4}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "CompanyName" "Hivemind"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "FileVersion" "${APP_VERSION}"
VIAddVersionKey "ProductVersion" "${APP_VERSION}"
VIAddVersionKey "LegalCopyright" "Hivemind"

!define MUI_ICON "${ICON_FILE}"
!define MUI_UNICON "${ICON_FILE}"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "TradChinese"
!insertmacro MUI_LANGUAGE "English"

LangString MSG_RUNNING ${LANG_TRADCHINESE} "${APP_NAME} 正在執行。請先結束它，再重新執行安裝程式。"
LangString MSG_RUNNING ${LANG_ENGLISH} "${APP_NAME} is running. Quit it, then run this installer again."

; Stop before touching files that a running instance holds open.
!macro StopIfRunning IMG
  nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq ${IMG}" /NH | find /I "${IMG}" >nul'
  Pop $0
  ${If} $0 == 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "$(MSG_RUNNING)"
    Abort
  ${EndIf}
!macroend

Section "Install"
  !insertmacro StopIfRunning "${APP_EXE}"
  !insertmacro StopIfRunning "${APP_HELPER_EXE}"

  SetOutPath "$INSTDIR"
  !include "${FILES_NSH}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\Hivemind"
  CreateShortCut "$SMPROGRAMS\Hivemind\${APP_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
  CreateShortCut "$SMPROGRAMS\Hivemind\Uninstall ${APP_NAME}.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Hivemind\${APP_ID}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "Publisher" "Hivemind"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "UninstallString" "$\"$INSTDIR\Uninstall.exe$\""
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  !insertmacro StopIfRunning "${APP_EXE}"
  !insertmacro StopIfRunning "${APP_HELPER_EXE}"

  !include "${UNINSTALL_NSH}"

  Delete "$INSTDIR\Uninstall.exe"
  ; Removes the folder only if it is empty, so .env files and sandbox data stay.
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\Hivemind\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\Hivemind\Uninstall ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\Hivemind"

  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
  DeleteRegKey HKCU "Software\Hivemind\${APP_ID}"
SectionEnd
