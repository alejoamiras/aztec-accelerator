; NSIS installer hooks for the Aztec Accelerator (Tauri v2 `bundle.windows.nsis.installerHooks`).
;
; POSTUNINSTALL removes the local CA from the CurrentUser Root store on a REAL uninstall.
;
; CRITICAL (audit R1 / C-1): Tauri runs the PREVIOUS version's uninstaller when installing over an
; existing install — i.e. on every upgrade. Without a guard this hook would delete the trust anchor +
; certs on every update, silently breaking HTTPS until the user re-enabled it. This must be correct in
; the FIRST release that ships the hook — a release cannot fix its own uninstaller.
;
; TWO guards are needed, because `$UpdateMode` alone does not cover every upgrade. In the Tauri NSIS
; template, `$UpdateMode` is set from the INSTALLER's own `/UPDATE` flag, and is forwarded to the old
; uninstaller only when the installer itself received it:
;
;     ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|}   ; append /UPDATE
;     StrCpy $R1 "$R1 _?=$4"                                         ; append uninstall directory
;
; The in-app updater passes `/UPDATE`. A user who downloads the new installer and double-clicks it —
; the only route available once auto-update is off — does NOT, so `$UpdateMode` is 0 all the way down
; and the old uninstaller wiped the trust anchor on a plain upgrade (post-impl codex Medium).
;
; `_?=` IS appended unconditionally on that path, and it is a reliable discriminator: the registered
; UninstallString is a bare `"$INSTDIR\uninstall.exe"`, so an Add/Remove-Programs (or double-clicked
; uninstall.exe) run never carries it. Present ⟹ an installer is driving us through an install-over.
; `${GetOptions}` takes the option's FIRST character as the switch marker, so this searches for the
; literal `_?=` outside quotes — `?` is illegal in Windows paths, so it cannot false-positive on the
; install directory, and the quoted exe path is skipped by its quote handling.
;
; Erring toward NOT deleting is the right bias: a leftover anchor is a CA whose private key was
; generated per-signature and discarded, never written to disk, and is name-constrained to loopback —
; nobody can mint a certificate with it, including us. A wrongly-deleted anchor, by contrast, breaks
; HTTPS for a user who did nothing but upgrade.
;
; Deletes by CN (not serial): at uninstall the app exe (and x509-parser) is gone, and rotation has
; already removed prior anchors, so only our single "Aztec Accelerator Local CA" remains (plan D4).

!macro NSIS_HOOK_POSTUNINSTALL
  Push $0
  Push $1
  StrCpy $1 0
  ClearErrors
  ${GetOptions} $CMDLINE "_?=" $0
  ${IfNot} ${Errors}
    StrCpy $1 1 ; an installer is running us as part of an install-over, not a real uninstall
  ${EndIf}
  ${If} $UpdateMode <> 1
  ${AndIf} $1 <> 1
    ; Absolute System32 certutil ($SYSDIR) — never a PATH lookup.
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root "Aztec Accelerator Local CA"'
    ; Remove the generated cert material from the user profile.
    RMDir /r "$PROFILE\.aztec-accelerator\certs"
  ${EndIf}
  Pop $1
  Pop $0
!macroend
