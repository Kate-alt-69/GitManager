; GitManager per-user installer customizations.
; The install directory is separate from user config and data.

!define MUI_FINISHPAGE_RUN_TEXT "Continue using GitManager"

!macro NSIS_HOOK_PREINSTALL
  StrCpy $INSTDIR "$LOCALAPPDATA\kastrick's_software\GitManager"
  CreateDirectory "$INSTDIR"
  SetOutPath "$INSTDIR"
!macroend

 
!include LogicLib.nsh

; Tauri invokes this after files, registry entries and shortcuts are installed.
; Keep CPython private to the current user and do not touch machine-wide Python.
!macro NSIS_HOOK_POSTINSTALL
  StrCpy $0 "$INSTDIR\resources\python\python.exe"
  IfFileExists "$0" 0 python_runtime_missing
  ; All bundled files have been extracted at this point. Verify the private runtime.
  nsExec::ExecToLog '"$0" -I -c "import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"'
  Pop $1
  StrCmp $1 "0" python_runtime_ready
  MessageBox MB_ICONSTOP "GitManager's private Python runtime could not be initialized. Please rerun the installer."
  Abort
python_runtime_missing:
  MessageBox MB_ICONSTOP "The GitManager installer is missing its bundled Python runtime. This package is incomplete."
  Abort
python_runtime_ready:
  DetailPrint "GitManager private Python runtime is installed and verified."
!macroend
