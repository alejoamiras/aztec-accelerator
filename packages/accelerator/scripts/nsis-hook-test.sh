#!/usr/bin/env bash
# Run the Windows NSIS uninstall hook on any machine with Docker — no Windows, no CI round-trip.
#
# The hook (src-tauri/nsis/hooks.nsi) decides whether to delete the user's local CA from the Windows
# trust store. It must skip that during an UPGRADE and do it on a real uninstall. A release cannot fix
# its own uninstaller, so getting it wrong ships permanently. The Windows leg of the `cert-trust` CI
# job runs exactly these cases on a real runner; this is the same thing under wine, in ~30s, so the
# hook can be iterated on locally instead of one 15-minute CI push at a time.
#
# It builds a throwaway image (wine + makensis), compiles harness.test.nsi around the REAL hook macro,
# and runs the resulting uninstaller under each command line, reporting how each was classified. The
# observable is the hook's own `RMDir` target: a seeded marker file survives iff the guard held.
#
#   bun run --cwd packages/accelerator test:nsis
set -euo pipefail

NSIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../src-tauri/nsis" && pwd)"
IMAGE="aztec-nsis-hook-test"

if ! docker info >/dev/null 2>&1; then
  echo "docker is required (it runs wine + makensis); the same cases run on the Windows CI leg" >&2
  exit 1
fi

# wine32 because NSIS stubs are x86. Cached after the first run.
docker build -q -t "$IMAGE" - >/dev/null <<'DOCKERFILE'
FROM debian:bookworm-slim
RUN dpkg --add-architecture i386 && apt-get update && \
    apt-get install -y --no-install-recommends wine wine32:i386 nsis ca-certificates && \
    rm -rf /var/lib/apt/lists/*
ENV WINEDEBUG=-all WINEPREFIX=/wp WINEDLLOVERRIDES=mscoree=d
RUN wineboot --init 2>/dev/null; wineserver -w || true
DOCKERFILE

# -i is required: without stdin attached, `bash -s` reads an empty script and exits 0 — a green run
# that tested nothing.
docker run --rm -i -v "$NSIS_DIR":/nsis:ro "$IMAGE" bash -s <<'RUNNER'
set -u
cp -r /nsis /work && cd /work
# Stub app exe first — harness.test.nsi bundles it so PREUNINSTALL has a real target to invoke.
makensis -V2 stub.test.nsi >/dev/null || { echo "makensis stub.test.nsi failed"; exit 1; }
makensis -V2 harness.test.nsi >/dev/null || { echo "makensis failed"; exit 1; }

PROFILE="$WINEPREFIX/drive_c/users/$(whoami)"
fails=0

# label | pass /UPDATE? | pass _?=<dir>? | must the anchor survive? | expect the F-05 breadcrumb?
run_case() {
  local label="$1" update="$2" in_place="$3" must_survive="$4" want_warning="${5:-no}"
  wine hooks-harness-setup.exe /S >/dev/null 2>&1
  local dir certs
  dir="$PROFILE/Temp/aztec-hooks-harness"
  [ -d "$dir" ] || { echo "!! install produced no directory"; return 1; }
  certs="$PROFILE/.aztec-accelerator/certs"
  mkdir -p "$certs"; echo seeded > "$certs/marker.txt"

  rm -f "$PROFILE/prepare-uninstall-invoked.txt"
  local args=(/S)
  [ "$update" = yes ] && args+=(/UPDATE)
  [ "$in_place" = yes ] && args+=("_?=$(winepath -w "$dir")")
  wine "$dir/uninstall.exe" "${args[@]}" >/dev/null 2>&1
  wineserver -w

  local survived=no
  [ -f "$certs/marker.txt" ] && survived=yes
  if [ "$survived" = "$must_survive" ]; then
    echo "ok   $label (anchor kept=$survived)"
  else
    echo "FAIL $label (anchor kept=$survived, expected=$must_survive)"
    fails=$((fails + 1))
  fi

  # PREUNINSTALL must INVOKE `--prepare-uninstall` on a REAL uninstall (guard passed) and be SKIPPED on an
  # upgrade — exactly when the anchor is removed. The stub app records that it ran; assert the two agree.
  local invoked=no want_invoked=yes
  [ -f "$PROFILE/prepare-uninstall-invoked.txt" ] && invoked=yes
  [ "$must_survive" = yes ] && want_invoked=no
  if [ "$invoked" = "$want_invoked" ]; then
    echo "ok   $label (prepare-uninstall invoked=$invoked)"
  else
    echo "FAIL $label (prepare-uninstall invoked=$invoked, expected=$want_invoked)"
    fails=$((fails + 1))
  fi
  rm -f "$PROFILE/prepare-uninstall-invoked.txt"

  # F-05 (audit 2026-07-31): the hook used to `ExecWait` certutil with NO output variable, so a
  # certutil that could not RUN was indistinguishable from a successful delete and the uninstall left
  # a trusted root behind with nothing recorded anywhere. Wine ships no certutil.exe, which makes this
  # container a faithful stand-in for the blocked-LOLBIN machine the finding is about: the hook must
  # now notice and leave the user a way to finish the job by hand. Pre-fix this file never appeared.
  local warned=no
  [ -f "$PROFILE/.aztec-accelerator/CA-TRUST-NOT-REMOVED.txt" ] && warned=yes
  if [ "$warned" = "$want_warning" ]; then
    echo "ok   $label (unremoved-CA warning=$warned)"
  else
    echo "FAIL $label (unremoved-CA warning=$warned, expected=$want_warning)"
    fails=$((fails + 1))
  fi
  if [ "$warned" = yes ] && ! grep -q certmgr.msc "$PROFILE/.aztec-accelerator/CA-TRUST-NOT-REMOVED.txt"; then
    echo "FAIL $label (warning does not tell the user how to remove it by hand)"
    fails=$((fails + 1))
  fi

  rm -rf "$PROFILE/.aztec-accelerator" "$dir"
}

# `_?=` is what the Tauri installer appends when it runs the PREVIOUS version's uninstaller —
# which in tauri-bundler 2.8.1 happens ONLY on the interactive plain-upgrade path (silent
# in-app updates skip the uninstall entirely: PageLeaveReinstall's update-mode guard). It is
# invisible to the script ($CMDLINE is stripped of it) — its effect is that the uninstaller runs in
# place instead of from a ~nsu*.tmp copy, which is what the hook actually keys on.
# The `warn` column is the F-05 observable: only the branch that ACTUALLY tries to remove trust can
# report a failure, so the two upgrade cases must stay silent — a warning there would mean the guard
# leaked. Wine has no certutil, so the genuine uninstall must warn.
#                                                          /UPDATE  _?=  survive  warn
run_case "upgrade via downloaded installer (no /UPDATE)"        no  yes  yes       no
run_case "defense-in-depth: /UPDATE (dead in 2.8.1 silent)"   yes  yes  yes       no
run_case "genuine uninstall (Add/Remove Programs)"              no   no   no      yes

# ── B5: a FOREIGN autostart Run value (a copied second install still using this user's shared CA) must
# make even a GENUINE uninstall LEAVE the trust anchor + certs. Before B5, POSTUNINSTALL's certutil ran
# UNCONDITIONALLY on the real-uninstall path and would have wiped the shared CA out from under copy B
# (codex's committed-bug finding). The native foreign flag now gates it. ──
run_key='HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run'
wine hooks-harness-setup.exe /S >/dev/null 2>&1
fdir="$PROFILE/Temp/aztec-hooks-harness"
fcerts="$PROFILE/.aztec-accelerator/certs"; mkdir -p "$fcerts"; echo seeded > "$fcerts/marker.txt"
# Point the Run value at ANOTHER install dir (foreign). Quoted single token = our owned format, but a
# different path ⇒ the native canonicalized compare says foreign ⇒ leave everything shared.
wine reg add "$run_key" /v "Aztec Accelerator" /t REG_SZ /d '"C:\Other Install\AztecAccelerator.exe"' /f >/dev/null 2>&1
wine "$fdir/uninstall.exe" /S >/dev/null 2>&1   # no _?= ⇒ real uninstall ($EXEDIR != $INSTDIR)
wineserver -w
if [ -f "$fcerts/marker.txt" ]; then
  echo "ok   foreign Run value ⇒ genuine uninstall LEAVES the shared CA"
else
  echo "FAIL foreign Run value: uninstall wrongly removed a copied install's shared CA"
  fails=$((fails + 1))
fi
wine reg delete "$run_key" /v "Aztec Accelerator" /f >/dev/null 2>&1
rm -rf "$PROFILE/.aztec-accelerator" "$fdir"

# ── POSTINSTALL completion-token cases (piece-2 plan §6 T2) ──
# The template invokes NSIS_HOOK_POSTINSTALL via !ifmacrodef, so a MISSPELLED macro is silently
# skipped and nothing errors — the positive case below is the only thing that catches that: the
# token MUST appear, carrying the exact nonce.
aztec_dir="$PROFILE/.aztec-accelerator"

# Positive must-fire: seeded handoff becomes the token, byte-for-byte, and the handoff is consumed.
mkdir -p "$aztec_dir"
printf 'nonce-abc123' > "$aztec_dir/update-txn"
wine hooks-harness-setup.exe /S >/dev/null 2>&1
wineserver -w
if [ -f "$aztec_dir/update-txn-done" ] && [ "$(cat "$aztec_dir/update-txn-done")" = "nonce-abc123" ] \
   && [ ! -f "$aztec_dir/update-txn" ]; then
  echo "ok   POSTINSTALL renames handoff -> token (nonce preserved)"
else
  echo "FAIL POSTINSTALL must-fire: token missing/wrong or handoff not consumed"
  fails=$((fails + 1))
fi

# Double fire (reinstall-over): the handoff was consumed, so a second run must be a no-op — token
# unchanged, no error, and no handoff resurrected.
wine hooks-harness-setup.exe /S >/dev/null 2>&1
wineserver -w
if [ "$(cat "$aztec_dir/update-txn-done" 2>/dev/null)" = "nonce-abc123" ] && [ ! -f "$aztec_dir/update-txn" ]; then
  echo "ok   POSTINSTALL double fire is idempotent"
else
  echo "FAIL POSTINSTALL double fire disturbed the token"
  fails=$((fails + 1))
fi
rm -rf "$aztec_dir" "$PROFILE/Temp/aztec-hooks-harness"

# No-handoff no-op: a fresh install (no update-txn) must create NO token.
wine hooks-harness-setup.exe /S >/dev/null 2>&1
wineserver -w
if [ ! -f "$aztec_dir/update-txn-done" ]; then
  echo "ok   POSTINSTALL no-op without a handoff (fresh install)"
else
  echo "FAIL POSTINSTALL created a token with no handoff present"
  fails=$((fails + 1))
fi
rm -rf "$aztec_dir" "$PROFILE/Temp/aztec-hooks-harness"

# ── B5: crash-recovery task-belt PARSER fixtures (AztecStrContains) ──
# The live schtasks query is Windows-CI-only, but the OWNERSHIP DECISION the belt keys off is a pure
# string test — exercised here under Wine with captured task XML (codex's two-layer strategy). The belt
# deletes the task ONLY when its <Command> is EXACTLY this install's; these fixtures pin ours-vs-foreign,
# the deceptive-prefix defence, the escaped-path safe-leave, and truncated/empty output.
results="$PROFILE/aztec-parser-results.txt"
rm -f "$results"
makensis -V2 parser.test.nsi >/dev/null || { echo "makensis parser.test.nsi failed"; exit 1; }
wine parser-test.exe /S >/dev/null 2>&1
wineserver -w
check_parser() {
  local label="$1" want="$2" got
  got="$(grep "^$label=" "$results" 2>/dev/null | cut -d= -f2 | tr -d '\r')"
  if [ "$got" = "$want" ]; then
    echo "ok   parser: $label => $got"
  else
    echo "FAIL parser: $label => '$got' (expected '$want')"
    fails=$((fails + 1))
  fi
}
check_parser ours 1
check_parser foreign 0
check_parser deceptive 0
check_parser escaped 0
check_parser truncated 0
check_parser empty 0
rm -f "$results"

[ "$fails" -eq 0 ] || { echo "$fails case(s) misclassified"; exit 1; }
echo "all cases classified correctly"
RUNNER
