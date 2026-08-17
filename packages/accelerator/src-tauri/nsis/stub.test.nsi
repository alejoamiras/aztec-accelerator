; A stand-in for the installed app exe, so the harness can prove NSIS_HOOK_PREUNINSTALL actually INVOKES
; `"$INSTDIR\AztecAccelerator.exe" --prepare-uninstall` (codex: the harness previously only compiled the
; macro). The real app is not built under Wine; this stub just records that it ran, into a marker the
; runner checks. NOT shipped.

Name "aztec-stub"
OutFile "AztecAccelerator.exe"
RequestExecutionLevel user
SilentInstall silent

Section
  FileOpen $0 "$PROFILE\prepare-uninstall-invoked.txt" w
  FileWrite $0 "invoked"
  FileClose $0
SectionEnd
