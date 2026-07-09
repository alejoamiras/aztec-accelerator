; NSIS installer hooks for the Aztec Accelerator (Tauri v2 `bundle.windows.nsis.installerHooks`).
;
; POSTUNINSTALL removes the local CA from the CurrentUser Root store on a REAL uninstall.
;
; CRITICAL (audit R1 / C-1): Tauri runs the PREVIOUS version's uninstaller when installing over an
; existing install — i.e. on every auto-update. Without the `$UpdateMode` guard this hook would delete
; the trust anchor + certs on every update, silently breaking HTTPS until the user re-enabled it. The
; guard makes the hook a no-op during an update (`$UpdateMode = 1`, set by the Tauri NSIS template when
; invoked with `/UPDATE`) and only fires on a genuine uninstall. This must be correct in the FIRST
; release that ships the hook — a release cannot fix its own uninstaller.
;
; Deletes by CN (not serial): at uninstall the app exe (and x509-parser) is gone, and rotation has
; already removed prior anchors, so only our single "Aztec Accelerator Local CA" remains (plan D4).

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; Absolute System32 certutil ($SYSDIR) — never a PATH lookup.
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root "Aztec Accelerator Local CA"'
    ; Remove the generated cert material from the user profile.
    RMDir /r "$PROFILE\.aztec-accelerator\certs"
  ${EndIf}
!macroend
