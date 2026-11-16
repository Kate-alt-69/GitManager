; GitManager per-user installer customizations.
; Product layout:
;   %LOCALAPPDATA%\kastrick's_software\GitManager
;   %LOCALAPPDATA%\kastrick's_software\config\GitManager
;   %LOCALAPPDATA%\kastrick's_software\data\GitManager

!define MUI_FINISHPAGE_RUN_TEXT "Continue using GitManager"

!macro NSIS_HOOK_PREINSTALL
  SetShellVarContext current
  StrCpy $INSTDIR "$LOCALAPPDATA\kastrick's_software\GitManager"
  StrCpy $0 "$LOCALAPPDATA\kastrick's_software"
  CreateDirectory "$INSTDIR"
  CreateDirectory "$0\config\GitManager"
  CreateDirectory "$0\data\GitManager"
  DetailPrint "GitManager install directory: $INSTDIR"
  SetOutPath "$INSTDIR"
!macroend

 
!include LogicLib.nsh

; Tauri invokes this after files, registry entries and shortcuts are installed.
; Keep CPython private to the current user and do not touch machine-wide Python.
!macro NSIS_HOOK_POSTINSTALL
  StrCmp $INSTDIR "$LOCALAPPDATA\kastrick's_software\GitManager" install_directory_valid install_directory_invalid
install_directory_invalid:
  MessageBox MB_ICONSTOP "GitManager was not installed into the required per-user directory: $LOCALAPPDATA\kastrick's_software\GitManager"
  Abort
install_directory_valid:
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
  DetailPrint "GitManager private Python runtime is installed and verified."
!macroend
