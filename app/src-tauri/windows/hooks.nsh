; Hooks for Tauri's NSIS installer.
;
; Before uninstalling, give claude:// and codex:// back to the apps and remove
; VibeLauncher's Start-menu shortcuts; otherwise sign-in links would point at a
; program that no longer exists. Profiles and their data are kept. Skipped
; when the uninstaller runs as part of an upgrade (/UPDATE).
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\vibelauncher.exe"
      ExecWait '"$INSTDIR\vibelauncher.exe" cleanup'
    ${EndIf}
  ${EndIf}
!macroend
