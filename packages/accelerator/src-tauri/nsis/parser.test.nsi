; Parser fixtures for the crash-recovery task belt's OWNERSHIP DECISION (codex's two-layer strategy: this
; runs under Wine with CAPTURED task XML; the live `schtasks` query is Windows-CI-only). NOT shipped.
;
; The belt deletes the task ONLY when the queried XML is COMPLETE (ends in </Task>), holds EXACTLY ONE
; action — one <Exec> with one <Command>, and NO <ComHandler>/<SendEmail>/<ShowMessage> — whose command is
; EXACTLY this install's. These fixtures pin: ours matches; a different/prefixed/escaped path does not; a
; MULTI-action or OTHER-action-type task does not (codex r2 #2); and truncated output does not.

!include LogicLib.nsh
!include "hooks.nsi"

Name "parser-test"
OutFile "parser-test.exe"
InstallDir "$TEMP\aztec-parser-test"
RequestExecutionLevel user
SilentInstall silent

!define EXPECT "<Command>C:\Install\AztecAccelerator.exe</Command>"

; Replicates the belt's decision on a fixture. $6 = scratch sub-result; $R0 = verdict; $9 = results file.
!macro DECIDE LABEL XML
  StrCpy $R0 "1"
  !insertmacro AztecStrContains $6 `${XML}` "</Task>"
  ${IfNot} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrCount $6 `${XML}` "<Exec>"
  ${IfNot} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrCount $6 `${XML}` "<Command>"
  ${IfNot} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrContains $6 `${XML}` "<ComHandler"
  ${If} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrContains $6 `${XML}` "<SendEmail"
  ${If} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrContains $6 `${XML}` "<ShowMessage"
  ${If} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  !insertmacro AztecStrContains $6 `${XML}` "${EXPECT}"
  ${IfNot} $6 == "1"
    StrCpy $R0 "0"
  ${EndIf}
  FileWrite $9 "${LABEL}=$R0$\r$\n"
!macroend

Section "Install"
  FileOpen $9 "$PROFILE\aztec-parser-results.txt" w
  ; ours: complete, one <Exec>/<Command>, ours ⇒ 1
  !insertmacro DECIDE "ours" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec></Actions></Task>"
  ; foreign: different install dir ⇒ 0
  !insertmacro DECIDE "foreign" "<Task><Actions><Exec><Command>C:\Other\AztecAccelerator.exe</Command></Exec></Actions></Task>"
  ; deceptive-prefix: C:\Install-evil is NOT C:\Install ⇒ 0
  !insertmacro DECIDE "deceptive" "<Task><Actions><Exec><Command>C:\Install-evil\AztecAccelerator.exe</Command></Exec></Actions></Task>"
  ; escaped special char: task stores &amp;, our needle has & ⇒ 0 (documented safe-leave)
  !insertmacro DECIDE "escaped" "<Task><Actions><Exec><Command>C:\A &amp; B\AztecAccelerator.exe</Command></Exec></Actions></Task>"
  ; multi-action: two <Exec> ⇒ 0
  !insertmacro DECIDE "multi" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec><Exec><Command>x</Command></Exec></Actions></Task>"
  ; other action types alongside ours ⇒ 0 (codex #2: all of ComHandler/SendEmail/ShowMessage)
  !insertmacro DECIDE "other_comhandler" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec><ComHandler><ClassId>x</ClassId></ComHandler></Actions></Task>"
  !insertmacro DECIDE "other_sendemail" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec><SendEmail><Server>x</Server></SendEmail></Actions></Task>"
  !insertmacro DECIDE "other_showmessage" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec><ShowMessage><Title>x</Title></ShowMessage></Actions></Task>"
  ; truncated AFTER our element (no </Task>) ⇒ 0
  !insertmacro DECIDE "truncated_after" "<Task><Actions><Exec><Command>C:\Install\AztecAccelerator.exe</Command></Exec>"
  ; truncated BEFORE the element closes ⇒ 0
  !insertmacro DECIDE "truncated" "<Task><Actions><Exec><Command>C:\Install\AztecAcceler"
  ; empty output ⇒ 0
  !insertmacro DECIDE "empty" ""
  FileClose $9
SectionEnd
