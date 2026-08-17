; Parser fixtures for `AztecStrContains` — the pure decision the crash-recovery task belt keys off
; (codex's two-layer strategy: this runs under Wine with CAPTURED task XML; the live `schtasks` query is
; Windows-CI-only). NOT shipped in any bundle. Built + run by the same `test:nsis` harness.
;
; The belt deletes the task ONLY when the queried XML contains the EXACT `<Command>` element THIS install
; would produce (`crash_recovery::task_xml` = `<Command>` + xml_escape(exe) + `</Command>`). These fixtures
; pin: our own element matches; a different or prefixed path does NOT; a special-char (escaped) path we
; cannot reproduce unescaped does NOT (safe-leave); truncated output does NOT.

!include LogicLib.nsh
!include "hooks.nsi"

Name "parser-test"
OutFile "parser-test.exe"
InstallDir "$TEMP\aztec-parser-test"
RequestExecutionLevel user
SilentInstall silent

!define EXPECT "<Command>C:\Install\AztecAccelerator.exe</Command>"

; $8 (not $0-$4, which AztecStrContains saves/restores as scratch) holds the result.
!macro CHECK LABEL XML
  !insertmacro AztecStrContains $8 `${XML}` "${EXPECT}"
  FileWrite $9 "${LABEL}=$8$\r$\n"
!macroend

Section "Install"
  FileOpen $9 "$PROFILE\aztec-parser-results.txt" w
  ; ours: identical element ⇒ 1
  !insertmacro CHECK "ours" "<Task><Command>C:\Install\AztecAccelerator.exe</Command></Task>"
  ; foreign: a different install dir ⇒ 0
  !insertmacro CHECK "foreign" "<Task><Command>C:\Other\AztecAccelerator.exe</Command></Task>"
  ; deceptive-prefix: C:\Install-evil is NOT C:\Install ⇒ 0 (full-element match, not prefix)
  !insertmacro CHECK "deceptive" "<Task><Command>C:\Install-evil\AztecAccelerator.exe</Command></Task>"
  ; escaped special char: the task stores &amp;, our unescaped needle has & ⇒ 0 (documented safe-leave)
  !insertmacro CHECK "escaped" "<Task><Command>C:\A &amp; B\AztecAccelerator.exe</Command></Task>"
  ; truncated output: element cut off ⇒ 0
  !insertmacro CHECK "truncated" "<Task><Command>C:\Install\AztecAcceler"
  ; empty output ⇒ 0
  !insertmacro CHECK "empty" ""
  FileClose $9
SectionEnd
