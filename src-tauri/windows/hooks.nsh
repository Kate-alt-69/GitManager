; GitManager per-user installer customizations.
; Product layout:
;   %LOCALAPPDATA%\kastrick's_software\GitManager
;   %LOCALAPPDATA%\kastrick's_software\config\GitManager
;   %LOCALAPPDATA%\kastrick's_software\data\GitManager

!include LogicLib.nsh

!define MUI_FINISHPAGE_RUN_TEXT "Continue using GitManager"

; The service is a detached per-user process and does not close with the UI.
; Stop only processes belonging to the interactive account, never every user's
; GitManager instance on a shared Windows machine.
!macro StopGitManagerPerUserProcesses
  SetShellVarContext current
  DetailPrint "Stopping GitManager's per-user service and supervisor before replacing files..."

  ; Prevent an optional logon task from relaunching the service mid-upgrade.
  ; Keep the task itself on install/update; it is user-configured state.
  nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /End /TN "GitManager Core Service"'
  Pop $R9

  ; The UI has already been closed by CheckIfAppIsRunning. Let crash_helper
  ; observe that exit first, then terminate a stale helper if it remains.
  Sleep 700
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "crash_helper.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "crash_helper-x86_64-pc-windows-msvc.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "crash_helper-i686-pc-windows-msvc.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "crash_helper-aarch64-pc-windows-msvc.exe"'
  Pop $R9
  Sleep 250

  ; Current dotted sidecar and historical Cargo executable names.
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "gitmanager.service.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "gitmanager.service-x86_64-pc-windows-msvc.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "gitmanager.service-i686-pc-windows-msvc.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "gitmanager.service-aarch64-pc-windows-msvc.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "gitmanager-service.exe"'
  Pop $R9
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /FI "USERNAME eq $%USERDOMAIN%\$%USERNAME%" /IM "git-manager-service.exe"'
  Pop $R9
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; The directory page and Tauri's .onInit have already selected $INSTDIR.
  ; Respect a user's Browse selection instead of replacing it here.
  !insertmacro StopGitManagerPerUserProcesses
  SetShellVarContext current
  StrCpy $0 "$LOCALAPPDATA\kastrick's_software"
  CreateDirectory "$INSTDIR"
  CreateDirectory "$0\config\GitManager"
  CreateDirectory "$0\data\GitManager"
  DetailPrint "GitManager install directory: $INSTDIR"
  SetOutPath "$INSTDIR"
!macroend

; Uninstall the optional logon task only for a real uninstall, not an update.
; Do not delete user data or the user's scan/index configuration.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopGitManagerPerUserProcesses
  ${If} $UpdateMode <> 1
    nsExec::ExecToLog '"$SYSDIR\schtasks.exe" /Delete /TN "GitManager Core Service" /F'
    Pop $R9
  ${EndIf}
!macroend

; Tauri invokes this after files, registry entries and shortcuts are installed.
; Keep CPython private to the current user and verify every required sidecar.
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$INSTDIR\crash_helper.exe" crash_helper_payload_ready 0
  IfFileExists "$INSTDIR\crash_helper-x86_64-pc-windows-msvc.exe" crash_helper_payload_ready 0
  IfFileExists "$INSTDIR\crash_helper-i686-pc-windows-msvc.exe" crash_helper_payload_ready 0
  IfFileExists "$INSTDIR\crash_helper-aarch64-pc-windows-msvc.exe" crash_helper_payload_ready 0
  MessageBox MB_ICONSTOP "GitManager's required crash_helper sidecar is missing from the installation. This package is incomplete; rebuild the installer with all sidecars."
  Abort
crash_helper_payload_ready:

  IfFileExists "$INSTDIR\gitmanager.service.exe" core_service_payload_ready 0
  IfFileExists "$INSTDIR\gitmanager.service-x86_64-pc-windows-msvc.exe" core_service_payload_ready 0
  IfFileExists "$INSTDIR\gitmanager.service-i686-pc-windows-msvc.exe" core_service_payload_ready 0
  IfFileExists "$INSTDIR\gitmanager.service-aarch64-pc-windows-msvc.exe" core_service_payload_ready 0
  MessageBox MB_ICONSTOP "GitManager's required core-service sidecar is missing from the installation. The repository engine cannot start; rebuild the installer with all sidecars."
  Abort
core_service_payload_ready:

  StrCpy $0 "$INSTDIR\resources\python\python.exe"
  DetailPrint "Verifying GitManager's bundled Python runtime: $0"
  IfFileExists "$0" 0 python_runtime_missing
  ; All bundled files have been extracted at this point. Verify the private runtime.
  nsExec::ExecToLog '"$0" -I -c "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"'
  Pop $1
  StrCmp $1 "0" python_runtime_ready
  MessageBox MB_ICONSTOP "GitManager's private Python runtime could not be initialized. Please rerun the installer."
  Abort
python_runtime_missing:
  MessageBox MB_ICONSTOP "The GitManager installer is missing its bundled Python runtime. Expected: $INSTDIR\resources\python\python.exe. This package is incomplete."
  Abort
python_runtime_ready:
  DetailPrint "GitManager sidecars and private Python runtime are installed and verified."
!macroend
