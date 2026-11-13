; GitManager per-user installer customizations.
; The install directory is separate from user config and data.

!define MUI_FINISHPAGE_RUN_TEXT "Continue using GitManager"
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Create a desktop shortcut"

!macro NSIS_HOOK_PREINSTALL
  StrCpy $INSTDIR "$LOCALAPPDATA\kastrick's_software\GitManager"
  CreateDirectory "$INSTDIR"
  SetOutPath "$INSTDIR"
!macroend
