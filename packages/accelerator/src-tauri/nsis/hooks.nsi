; NSIS installer hooks for the Aztec Accelerator (Tauri v2 `bundle.windows.nsis.installerHooks`).
;
; POSTUNINSTALL removes the local CA from the CurrentUser Root store on a REAL uninstall.
;
; CRITICAL (audit R1 / C-1): Tauri runs the PREVIOUS version's uninstaller when a user installs the
; new version INTERACTIVELY over an existing install (wizard plain upgrade — silent in-app updates
; skip it entirely; see the measured note below). Without a guard this hook would delete the trust
; anchor + certs on that upgrade, silently breaking HTTPS until the user re-enabled it. This must be
; correct in the FIRST release that ships the hook — a release cannot fix its own uninstaller.
;
; ONE guard is load-bearing in 2.8.1 — `$EXEDIR != $INSTDIR` — because the only path that runs this
; hook during an upgrade is the interactive one, where `$UpdateMode` is 0. `$UpdateMode` is kept as
; defense-in-depth for a future template that forwards `/UPDATE`. In the Tauri NSIS
; template, `$UpdateMode` is set from the INSTALLER's own `/UPDATE` flag, and is forwarded to the old
; uninstaller only when the installer itself received it:
;
;     ${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 "$R1 /UPDATE" ${|}   ; append /UPDATE
;     StrCpy $R1 "$R1 _?=$4"                                         ; append uninstall directory
;
; The in-app updater passes `/UPDATE` — but in tauri-bundler 2.8.1 that line is DEAD CODE:
; PageLeaveReinstall's update-mode guard (`${If} $UpdateMode = 1` → reinst_done) skips
; reinst_uninstall entirely, so the SILENT in-app update never runs the old uninstaller at all
; (measured live: smoke run 30472678896 — a sentinel baked into the installed N-1's uninstaller,
; armed, never fired while the update completed). The `/UPDATE` handling below is kept as
; defense-in-depth for a future template that forwards it. The path that DOES run the old
; uninstaller is the INTERACTIVE plain upgrade: a user double-clicks the new installer — the only
; route once auto-update is off — `$UpdateMode` is 0 all the way down, and the old uninstaller
; wiped the trust anchor on that upgrade (post-impl codex Medium).
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
;   uninstall.exe /S /UPDATE _?=<dir>    $EXEDIR = <dir>              == $INSTDIR   install-over (†)
;   uninstall.exe /S   (Add/Remove)      $EXEDIR = ...\~nsu1.tmp      != $INSTDIR   REAL uninstall
;
; (†) constructed from installer.nsi:333, which 2.8.1 never reaches in update mode (dead code) —
;     no silent in-app update invokes the uninstaller. Row kept as defense-in-depth.
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

; ── POSTUNINSTALL and F-05 (audit 2026-07-31): removal must not fail silently ──
;
; `ExecWait` WITHOUT an output variable discards the exit code, so a `certutil` that never ran (a
; commonly restricted LOLBIN — EDR/AppLocker block it) or that ran and failed was indistinguishable
; from a clean delete: the user's uninstall left a trusted root behind and NOTHING recorded it
; anywhere. Silent uninstalls show no details window, so `DetailPrint` alone would be unread.
;
; The verdict is taken the same way `trust/windows.rs` takes it — ASK AGAIN afterwards rather than
; interpreting the delete's return code. `certutil -store` exits 0 only when it FOUND the anchor, so
; "exit 0" means still-present and a launch failure means could-not-determine. That deliberately
; avoids whitelisting `CRYPT_E_NOT_FOUND` by numeric value: "nothing to delete" is the NORMAL result
; on every machine that never enabled HTTPS, and mistaking it for a failure would fire a scary
; breadcrumb on virtually every uninstall — strictly worse than the silence it replaces.
!macro NSIS_HOOK_POSTUNINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
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
    ; A warning from a previous uninstall must not outlive it.
    Delete "$PROFILE\.aztec-accelerator\CA-TRUST-NOT-REMOVED.txt"
    ; Absolute System32 certutil ($SYSDIR) — never a PATH lookup.
    ExecWait '"$SYSDIR\certutil.exe" -user -delstore Root "Aztec Accelerator Local CA"' $2
    ; Then confirm, instead of trusting the delete's own word for it.
    StrCpy $4 ""
    ${If} $2 == "error"
      StrCpy $4 "certutil.exe could not be launched, so the certificate was never removed."
    ${Else}
      ExecWait '"$SYSDIR\certutil.exe" -user -store Root "Aztec Accelerator Local CA"' $3
      ${If} $3 == "error"
        StrCpy $4 "certutil.exe could not be launched, so removal could not be confirmed."
      ${ElseIf} $3 = 0
        StrCpy $4 "the certificate is still present in the store (certutil -delstore exited $2)."
      ${EndIf}
    ${EndIf}
    ${If} $4 != ""
      DetailPrint "aztec: the local CA was NOT removed from your trust store — $4"
      ClearErrors
      FileOpen $5 "$PROFILE\.aztec-accelerator\CA-TRUST-NOT-REMOVED.txt" w
      ${IfNot} ${Errors}
        FileWrite $5 "Aztec Accelerator could not remove its local certificate authority.$\r$\n$\r$\n"
        FileWrite $5 "Reason: $4$\r$\n$\r$\n"
        FileWrite $5 "The certificate 'Aztec Accelerator Local CA' may still be trusted by this$\r$\n"
        FileWrite $5 "account. To remove it by hand: press Win+R, run certmgr.msc, open$\r$\n"
        FileWrite $5 "'Trusted Root Certification Authorities' > 'Certificates', find$\r$\n"
        FileWrite $5 "'Aztec Accelerator Local CA' and delete it.$\r$\n$\r$\n"
        FileWrite $5 "(That CA's private key was generated per-signature and never written to disk,$\r$\n"
        FileWrite $5 "and it is name-constrained to loopback addresses, so it cannot be used to$\r$\n"
        FileWrite $5 "issue certificates for real websites.)$\r$\n"
        FileClose $5
      ${EndIf}
    ${EndIf}
    ; Remove the generated cert material from the user profile.
    RMDir /r "$PROFILE\.aztec-accelerator\certs"
  ${EndIf}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
