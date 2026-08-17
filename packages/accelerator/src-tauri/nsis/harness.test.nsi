; Test harness for `hooks.nsi` — NOT shipped in any bundle. Built and run by the Windows leg of the
; `cert-trust` CI job (.github/workflows/accelerator.yml).
;
; The POSTUNINSTALL hook has a property no other test can reach: it must be correct in the FIRST
; release that ships it, because a release cannot fix its own uninstaller. Getting it wrong wipes the
; user's trust anchor on a routine upgrade. So the guard is exercised for real here — a genuine NSIS
; uninstaller, built around the ACTUAL hook macro (included verbatim, never re-implemented), run on
; Windows under each of the three command lines it must tell apart (one of them — /UPDATE — is
; dead code in 2.8.1 silent updates and pinned as defense-in-depth; see hooks.nsi header).
;
; Observing the decision without touching a real trust store: the harness watches the hook's OTHER
; side effect, `RMDir /r "$PROFILE\.aztec-accelerator\certs"`. CI seeds a marker file in that directory
; before each run; whether it survives is exactly the guard's verdict. The `certutil -delstore` in the
; same branch is a no-op on a runner that has no such CA installed.
;
; Mirrors the Tauri NSIS template's context: `Var UpdateMode`, parsed from `/UPDATE` in `un.onInit`.

!include FileFunc.nsh
!include LogicLib.nsh

Var UpdateMode

; The Tauri NSIS template `!define`s this from `mainBinaryName` (our `[[bin]]` = AztecAccelerator). The
; PREUNINSTALL invocation and the POSTUNINSTALL native belt reference it, so mirror it here or makensis
; fails on an undefined `${MAINBINARYNAME}`.
!define MAINBINARYNAME "AztecAccelerator"

!include "hooks.nsi"

Name "hooks-harness"
OutFile "hooks-harness-setup.exe"
InstallDir "$TEMP\aztec-hooks-harness"
RequestExecutionLevel user
SilentInstall silent

Section "Install"
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  ; The stand-in app exe, so PREUNINSTALL's `ExecWait "$INSTDIR\AztecAccelerator.exe" --prepare-uninstall`
  ; has a real target to invoke (compiled first by the runner). It records that it ran.
  File "AztecAccelerator.exe"
  ; Mirrors the Tauri template: POSTINSTALL fires as the LAST act of Section Install
  ; (installer.nsi:709-711 in tauri-bundler 2.8.1, !ifmacrodef-guarded).
  !insertmacro NSIS_HOOK_POSTINSTALL
SectionEnd

Function un.onInit
  ${GetOptions} $CMDLINE "/UPDATE" $UpdateMode
  ${IfNot} ${Errors}
    StrCpy $UpdateMode 1
  ${EndIf}
FunctionEnd

Section "Uninstall"
  ; PREUNINSTALL first (as the Tauri template invokes it, before the exe is deleted). Its `ExecWait` of a
  ; nonexistent `$INSTDIR\AztecAccelerator.exe` is a harmless no-op under the harness — we exercise its
  ; GUARD (the same $EXEDIR/$INSTDIR discriminator the cert-trust cases drive) and that it compiles.
  !insertmacro NSIS_HOOK_PREUNINSTALL
  !insertmacro NSIS_HOOK_POSTUNINSTALL
SectionEnd
