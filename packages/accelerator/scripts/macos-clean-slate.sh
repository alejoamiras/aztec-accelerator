#!/usr/bin/env bash
#
# Return this Mac to a genuine first-run state for the accelerator: quit the running app, then delete
# every trace it persists — config, certificates, logs, the trusted CA anchor, and both LaunchAgents.
#
# Why a script: doing this by hand is easy to get subtly wrong, and a half-clean machine produces
# confusing test results. Two traps in particular:
#   - The app is a TRAY app. Closing its windows does not quit it, and a still-running instance holds
#     :59833/:59834 and can rewrite config while you delete it. So we quit first and verify.
#   - Rotation can leave SEVERAL anchors under the same common name. A single `delete-certificate`
#     removes one, so we loop until the keychain reports none left.
#
# Only ever touches paths this app owns. Usage:
#   ./macos-clean-slate.sh        # prompts before deleting
#   ./macos-clean-slate.sh -y     # no prompt
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script is macOS-only (it drives the login Keychain and LaunchAgents)." >&2
  exit 1
fi

APP_NAME="Aztec Accelerator"
BUNDLE_ID="dev.aztec.accelerator"
CA_CN="Aztec Accelerator Local CA"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

STATE_DIR="$HOME/.aztec-accelerator"
LOG_DIR="$HOME/Library/Application Support/aztec-accelerator"
AUTOSTART_PLIST="$HOME/Library/LaunchAgents/$BUNDLE_ID.plist"
RECOVERY_PLIST="$HOME/Library/LaunchAgents/$APP_NAME.plist"

assume_yes=false
[[ "${1:-}" == "-y" || "${1:-}" == "--yes" ]] && assume_yes=true

echo "This will delete, if present:"
echo "  • $STATE_DIR            (config + certificates)"
echo "  • $LOG_DIR              (logs)"
echo "  • every \"$CA_CN\" anchor in your login Keychain"
echo "  • $AUTOSTART_PLIST"
echo "  • $RECOVERY_PLIST"
echo
if [[ "$assume_yes" != true ]]; then
  read -r -p "Proceed? [y/N] " reply
  [[ "$reply" == "y" || "$reply" == "Y" ]] || { echo "Aborted."; exit 0; }
fi

# ── 1. Quit the app ──────────────────────────────────────────────────────────
echo "==> Quitting $APP_NAME"
osascript -e "quit app \"$APP_NAME\"" >/dev/null 2>&1 || true
sleep 1
pkill -f "$APP_NAME" >/dev/null 2>&1 || true
sleep 1

# The listeners are the honest signal that it is really gone.
if lsof -nP -iTCP:59833 -iTCP:59834 -sTCP:LISTEN >/dev/null 2>&1; then
  echo "   WARNING: something is still listening on :59833/:59834." >&2
  lsof -nP -iTCP:59833 -iTCP:59834 -sTCP:LISTEN >&2 || true
  echo "   Quit it before trusting these results." >&2
else
  echo "   ports clear"
fi

# ── 2. Launch agents (before file removal — stop anything that could relaunch) ─
for plist in "$AUTOSTART_PLIST" "$RECOVERY_PLIST"; do
  if [[ -f "$plist" ]]; then
    launchctl unload "$plist" >/dev/null 2>&1 || true
    rm -f "$plist"
    echo "==> Removed $(basename "$plist")"
  fi
done

# ── 3. Config, certificates, logs ────────────────────────────────────────────
for dir in "$STATE_DIR" "$LOG_DIR"; do
  if [[ -d "$dir" ]]; then
    rm -rf "$dir"
    echo "==> Removed $dir"
  fi
done

# ── 4. Every trusted CA anchor (loop: rotation can leave more than one) ──────
echo "==> Removing \"$CA_CN\" anchors from the login Keychain"
removed=0
for _ in $(seq 1 32); do
  security find-certificate -c "$CA_CN" "$KEYCHAIN" >/dev/null 2>&1 || break
  # -t also drops the user TRUST SETTINGS, not just the keychain item; without it an orphaned
  # trust entry survives that `find-certificate` can no longer see.
  security delete-certificate -t -c "$CA_CN" "$KEYCHAIN" >/dev/null 2>&1 || break
  removed=$((removed + 1))
done
echo "   removed $removed anchor(s)"

# ── 5. Verify ────────────────────────────────────────────────────────────────
echo
echo "==> Verifying"
fail=0
[[ -e "$STATE_DIR" ]] && { echo "   STILL PRESENT: $STATE_DIR" >&2; fail=1; }
[[ -e "$AUTOSTART_PLIST" ]] && { echo "   STILL PRESENT: $AUTOSTART_PLIST" >&2; fail=1; }
[[ -e "$RECOVERY_PLIST" ]] && { echo "   STILL PRESENT: $RECOVERY_PLIST" >&2; fail=1; }
if security find-certificate -c "$CA_CN" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "   STILL TRUSTED: a \"$CA_CN\" anchor remains in the login Keychain" >&2
  echo "   (check other keychains: security list-keychains)" >&2
  fail=1
fi

if [[ "$fail" -eq 0 ]]; then
  echo "   clean — this Mac now looks like it has never run the accelerator"
  echo
  echo "Next:"
  echo "  cd packages/accelerator && bunx tauri build --debug"
  echo "  open \"src-tauri/target/debug/bundle/macos/$APP_NAME.app\""
else
  echo
  echo "Not fully clean — see above." >&2
  exit 1
fi
