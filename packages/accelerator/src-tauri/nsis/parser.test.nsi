; Parser fixtures for the crash-recovery task belt's OWNERSHIP DECISION (codex's two-layer strategy: this
; runs under Wine with CAPTURED task XML; the live `schtasks` query is Windows-CI-only). NOT shipped.
;
; The belt deletes the task ONLY when the queried XML is COMPLETE (ends in </Task>), holds EXACTLY ONE
; action (<Command>), and that command is EXACTLY this install's — the same three tests
; `AztecDeleteOwnedRecoveryTask` applies. These fixtures pin: ours matches; a different/prefixed/escaped
; path does not; a MULTI-action task does not (codex: a task may hold up to 32 actions); and a truncated
; definition — even one truncated AFTER our element — does not (completeness catches it).

!include LogicLib.nsh
!include "hooks.nsi"

Name "parser-test"
OutFile "parser-test.exe"
InstallDir "$TEMP\aztec-parser-test"
RequestExecutionLevel user
SilentInstall silent

!define EXPECT "<Command>C:\Install\AztecAccelerator.exe</Command>"

; Replicates the belt's decision (complete AND exactly-one-Command AND ours) on a fixture. $6/$7/$8 hold
; the sub-results (not $0-$5, which AztecStrContains/AztecStrCount use as scratch); $R0 the verdict.
!macro DECIDE LABEL XML
  !insertmacro AztecStrContains $6 `${XML}` "</Task>"
  !insertmacro AztecStrCount $7 `${XML}` "<Command>"
  !insertmacro AztecStrContains $8 `${XML}` "${EXPECT}"
  StrCpy $R0 "0"
  ${If} $6 == "1"
  ${AndIf} $7 == "1"
  ${AndIf} $8 == "1"
    StrCpy $R0 "1"
  ${EndIf}
  FileWrite $9 "${LABEL}=$R0$\r$\n"
!macroend

Section "Install"
  FileOpen $9 "$PROFILE\aztec-parser-results.txt" w
  ; ours: complete, one command, ours ⇒ 1
  !insertmacro DECIDE "ours" "<Task><Command>C:\Install\AztecAccelerator.exe</Command></Task>"
  ; foreign: different install dir ⇒ 0
  !insertmacro DECIDE "foreign" "<Task><Command>C:\Other\AztecAccelerator.exe</Command></Task>"
  ; deceptive-prefix: C:\Install-evil is NOT C:\Install ⇒ 0
  !insertmacro DECIDE "deceptive" "<Task><Command>C:\Install-evil\AztecAccelerator.exe</Command></Task>"
  ; escaped special char: task stores &amp;, our needle has & ⇒ 0 (documented safe-leave)
  !insertmacro DECIDE "escaped" "<Task><Command>C:\A &amp; B\AztecAccelerator.exe</Command></Task>"
  ; multi-action: ours PLUS another action ⇒ 0 (count != 1 — codex #4)
  !insertmacro DECIDE "multi" "<Task><Command>C:\Install\AztecAccelerator.exe</Command><Command>C:\Evil\x.exe</Command></Task>"
  ; truncated AFTER our element (no </Task>) ⇒ 0 (completeness — codex #4)
  !insertmacro DECIDE "truncated_after" "<Task><Command>C:\Install\AztecAccelerator.exe</Command>"
  ; truncated BEFORE the element closes ⇒ 0
  !insertmacro DECIDE "truncated" "<Task><Command>C:\Install\AztecAcceler"
  ; empty output ⇒ 0
  !insertmacro DECIDE "empty" ""
  FileClose $9
SectionEnd
