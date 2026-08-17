#!/usr/bin/env bash
# Uninstall cleanup for macOS (.app) and Linux (.deb / AppImage), which have no NSIS-style uninstall hook.
# Runs the app's own ownership-checked `--prepare-uninstall` (removes autostart + crash-recovery + — only
# if THIS install owns them — the CA trust and generated certs), then tells you how to delete the app.
#
# It NEVER deletes the app bundle or your config/approved-origins itself — teardown of shared state is the
# only thing that needs a running binary; removing files you can see is left to you.
#
#   ./uninstall.sh                 # locate the binary automatically
#   ./uninstall.sh /path/to/exe    # or point it at the installed binary / AppImage
set -euo pipefail

BIN="${1:-}"

if [ -z "$BIN" ]; then
  # Common install locations first, then PATH.
  candidates=(
    "/Applications/Aztec Accelerator.app/Contents/MacOS/AztecAccelerator"
    "$HOME/Applications/Aztec Accelerator.app/Contents/MacOS/AztecAccelerator"
    "/usr/bin/AztecAccelerator"
    "/usr/local/bin/AztecAccelerator"
    "/opt/Aztec Accelerator/aztec-accelerator"
  )
  for c in "${candidates[@]}"; do
    [ -x "$c" ] && BIN="$c" && break
  done
  [ -z "$BIN" ] && BIN="$(command -v AztecAccelerator 2>/dev/null || true)"
fi

if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  cat >&2 <<EOF
Could not find the Aztec Accelerator binary automatically.
Pass its path (or the AppImage) explicitly, e.g.:
  ./uninstall.sh "/Applications/Aztec Accelerator.app/Contents/MacOS/AztecAccelerator"
  ./uninstall.sh ~/Downloads/AztecAccelerator.AppImage
EOF
  exit 2
fi

echo "Running ownership-checked cleanup: $BIN --prepare-uninstall"
if "$BIN" --prepare-uninstall; then
  echo
  echo "Cleanup complete. You can now delete the app:"
  echo "  macOS:   drag 'Aztec Accelerator.app' to the Trash"
  echo "  .deb:    sudo apt-get remove aztec-accelerator"
  echo "  AppImage: delete the .AppImage file"
  echo
  echo "Your config and approved origins in ~/.aztec-accelerator were left untouched."
else
  status=$?
  echo >&2
  echo "Cleanup reported a problem (exit $status) — see the lines above. The app was NOT deleted." >&2
  exit "$status"
fi
