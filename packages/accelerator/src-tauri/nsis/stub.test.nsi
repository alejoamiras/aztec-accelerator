; A stand-in for the installed app exe, so the harness can prove NSIS_HOOK_PREUNINSTALL actually INVOKES
; `"$INSTDIR\AztecAccelerator.exe" --prepare-uninstall` (codex: the harness previously only compiled the
; macro). It records invocation ONLY when the `--prepare-uninstall` argument is present — proving the
; argument, not merely that something ran. The real app is not built under Wine; this is a stub. NOT shipped.

!include FileFunc.nsh
!include LogicLib.nsh

Name "aztec-stub"
OutFile "AztecAccelerator.exe"
RequestExecutionLevel user
SilentInstall silent

Section
  ${GetOptions} $CMDLINE "--prepare-uninstall" $0
  ${IfNot} ${Errors}
    FileOpen $0 "$PROFILE\prepare-uninstall-invoked.txt" w
    FileWrite $0 "invoked"
    FileClose $0
  ${EndIf}
SectionEnd
