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
makensis -V2 harness.test.nsi >/dev/null || { echo "makensis failed"; exit 1; }

PROFILE="$WINEPREFIX/drive_c/users/$(whoami)"
fails=0

# label | pass /UPDATE? | pass _?=<dir>? | must the anchor survive?
run_case() {
  local label="$1" update="$2" in_place="$3" must_survive="$4"
  wine hooks-harness-setup.exe /S >/dev/null 2>&1
  local dir certs
  dir="$PROFILE/Temp/aztec-hooks-harness"
  [ -d "$dir" ] || { echo "!! install produced no directory"; return 1; }
  certs="$PROFILE/.aztec-accelerator/certs"
  mkdir -p "$certs"; echo seeded > "$certs/marker.txt"

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
  rm -rf "$PROFILE/.aztec-accelerator" "$dir"
}

# `_?=` is what the Tauri installer appends when it runs the PREVIOUS version's uninstaller —
# which in tauri-bundler 2.8.1 happens ONLY on the interactive plain-upgrade path (silent
# in-app updates skip the uninstall entirely: PageLeaveReinstall's update-mode guard). It is
# invisible to the script ($CMDLINE is stripped of it) — its effect is that the uninstaller runs in
# place instead of from a ~nsu*.tmp copy, which is what the hook actually keys on.
#                                                          /UPDATE  _?=  survive
run_case "upgrade via downloaded installer (no /UPDATE)"        no  yes  yes
run_case "upgrade via in-app updater (/UPDATE)"                yes  yes  yes
run_case "genuine uninstall (Add/Remove Programs)"              no   no   no

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

[ "$fails" -eq 0 ] || { echo "$fails case(s) misclassified"; exit 1; }
echo "all cases classified correctly"
RUNNER
