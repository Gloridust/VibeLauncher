; Hooks for Tauri's NSIS installer.
;
; Before uninstalling, give claude:// and codex:// back to the apps and remove
; VibeLauncher's Start-menu shortcuts; otherwise sign-in links would point at a
; program that no longer exists. Profiles and their data are kept.
;
; Installing a new version over an old one usually runs the old uninstaller
; first (without /UPDATE), so that cleanup also happens on an upgrade; after
; installing, `internal-restore` puts the links and shortcuts back for the
; profiles that are still there. On a fresh install it does nothing.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\vibelauncher.exe"
      ExecWait '"$INSTDIR\vibelauncher.exe" cleanup'
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} ${FileExists} "$INSTDIR\vibelauncher.exe"
    ExecWait '"$INSTDIR\vibelauncher.exe" internal-restore'
  ${EndIf}
!macroend
