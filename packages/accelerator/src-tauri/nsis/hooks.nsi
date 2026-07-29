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
; `_?=` IS appended unconditionally on that path — but it CANNOT be detected from `$CMDLINE`. The NSIS
; stub consumes it: invoked as `uninstall.exe /S _?=C:\dir`, the script sees `$CMDLINE` =
; `"...\uninstall.exe" /S`. Measured under wine, not assumed (an earlier `${GetOptions} $CMDLINE "_?="`
; guard shipped on exactly that wrong assumption and CI caught it).
;
; What `_?=` actually MEANS is the observable: "do not copy yourself to a temp dir before running".
; So WHERE the uninstaller is executing from is the discriminator, and it holds in all three cases:
;
;   uninstall.exe /S _?=<dir>            $EXEDIR = <dir>              == $INSTDIR   install-over
;   uninstall.exe /S /UPDATE _?=<dir>    $EXEDIR = <dir>              == $INSTDIR   install-over
;   uninstall.exe /S   (Add/Remove)      $EXEDIR = ...\~nsu1.tmp      != $INSTDIR   REAL uninstall
;
; `$UpdateMode` is kept as a second, independent guard: it costs nothing and still covers a future
; Tauri that forwards `/UPDATE` without `_?=`.
;
; Erring toward NOT deleting is the right bias: a leftover anchor is a CA whose private key was
; generated per-signature and discarded, never written to disk, and is name-constrained to loopback —
; nobody can mint a certificate with it, including us. A wrongly-deleted anchor, by contrast, breaks
; HTTPS for a user who did nothing but upgrade.
;
; Deletes by CN (not serial): at uninstall the app exe (and x509-parser) is gone, and rotation has
; already removed prior anchors, so only our single "Aztec Accelerator Local CA" remains (plan D4).

; ── POSTINSTALL: the update-window completion token (piece-2 plan §3, D21) ──
;
; The in-app updater cannot know when NSIS has finished: on Windows `install()` hands off and the
; process exits, so it writes an `update-in-progress.json` marker (plus a one-line nonce handoff at
; `update-txn`) and the NEXT launch refuses to touch autostart until the install demonstrably
; completed. This hook is that proof: the Tauri template invokes NSIS_HOOK_POSTINSTALL as the LAST
; act of `Section Install` — after every file copy, the uninstaller write, registry and shortcuts —
; so renaming the handoff into `update-txn-done` hands the nonce back exactly when "NSIS finished
; copying files" is true.
;
; Rename, not read+write: it preserves the nonce bytes with no FileRead loop, and it CONSUMES the
; handoff, which makes a double fire (reinstall-over) naturally idempotent. NSIS `Rename` fails if
; the destination exists, hence the Delete first. No handoff file ⇒ no-op — fresh installs never
; have one. (A failed prior update can leave a handoff that a later MANUAL install consumes; that
; is safe by BINDING, not construction: the token's nonce only matches its own marker, and removal
; still requires version + path. See plan §3.)
;
; The template guard is `!ifmacrodef` — a misspelled macro name is SILENTLY skipped. The harness's
; positive must-fire case (nsis-hook-test.sh) exists precisely to catch that.
!macro NSIS_HOOK_POSTINSTALL
  IfFileExists "$PROFILE\.aztec-accelerator\update-txn" 0 aztec_postinstall_done
    Delete "$PROFILE\.aztec-accelerator\update-txn-done"
    ClearErrors
    Rename "$PROFILE\.aztec-accelerator\update-txn" "$PROFILE\.aztec-accelerator\update-txn-done"
    ${If} ${Errors}
      ; Leave the handoff in place: no token appears, the marker suppresses until its deadline —
      ; the documented stranded exit. Never abort the install over bookkeeping.
      DetailPrint "aztec: update completion token could not be written"
    ${EndIf}
  aztec_postinstall_done:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Push $0
  Push $1
  ; Normalize both to canonical short (8.3) form before comparing. `$INSTDIR` is restored from the
  ; installer's registry value while `$EXEDIR` is derived from the launched path, so one directory can
  ; be spelled two ways (casing, trailing slash, long vs short name). A purely textual mismatch would
  ; read as "real uninstall" and delete — the unsafe direction — so canonicalize first, and fall back
  ; to the raw value only if the path can no longer be resolved.
  ClearErrors
  GetFullPathName /SHORT $0 "$EXEDIR"
  ${If} ${Errors}
    StrCpy $0 "$EXEDIR"
  ${EndIf}
  ClearErrors
  GetFullPathName /SHORT $1 "$INSTDIR"
  ${If} ${Errors}
    StrCpy $1 "$INSTDIR"
  ${EndIf}
  ${If} $UpdateMode <> 1
  ${AndIf} $0 != $1
    ; Absolute System32 certutil ($SYSDIR) — never a PATH lookup.
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root "Aztec Accelerator Local CA"'
    ; Remove the generated cert material from the user profile.
    RMDir /r "$PROFILE\.aztec-accelerator\certs"
  ${EndIf}
  Pop $1
  Pop $0
!macroend
