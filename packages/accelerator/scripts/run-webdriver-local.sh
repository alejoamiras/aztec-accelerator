#!/usr/bin/env bash
# Local WebDriver E2E: builds the webdriver-featured app, supervises it (and, when needed, an
# owned Xvfb/DBus/stalonetray stack mirroring _e2e-webdriver.yml), runs the WDIO suite, and tears
# down ONLY what it started. Fails fast when the host cannot provide a display — install
#   sudo apt install xvfb stalonetray dbus-x11
# or defer the suite to CI (see the plan's deferred-with-consent contingency).
set -euo pipefail

PKG_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$PKG_DIR"

OWNED_PGIDS=()
cleanup() {
  for pgid in "${OWNED_PGIDS[@]:-}"; do
    [ -n "$pgid" ] && kill -- "-$pgid" 2>/dev/null || true
  done
}
trap cleanup EXIT INT TERM

port_free() {
  ! (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

require_port() {
  if ! port_free "$1"; then
    echo "ERROR: port $1 is already in use (a real accelerator or another run?):" >&2
    ss -ltnp "sport = :$1" 2>/dev/null || true
    exit 1
  fi
}

# ── Display preflight ──
if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
  if ! command -v Xvfb >/dev/null || ! command -v stalonetray >/dev/null; then
    echo "ERROR: no display and Xvfb/stalonetray are not installed." >&2
    echo "Fix: sudo apt install xvfb stalonetray dbus-x11   (then rerun)" >&2
    echo "Or defer this suite to CI at PR time (deferred-with-consent)." >&2
    exit 2
  fi
  echo "No display: starting owned Xvfb + DBus + stalonetray (as CI does)"
  setsid Xvfb :99 -screen 0 1280x800x24 >/dev/null 2>&1 &
  OWNED_PGIDS+=("$!")
  export DISPLAY=:99
  sleep 1
  if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ] && command -v dbus-launch >/dev/null; then
    eval "$(dbus-launch --sh-syntax)"
    export DBUS_SESSION_BUS_ADDRESS
  fi
  setsid stalonetray >/dev/null 2>&1 &
  OWNED_PGIDS+=("$!")
fi

# ── Port preflight (4445 = tauri-plugin-webdriver, 59833 = the app's server) ──
require_port 4445
require_port 59833

# ── Build with the webdriver feature ──
echo "Building webdriver-featured app (this can take a while cold)..."
bun run frontend:build
( cd src-tauri && cargo build --features webdriver )

BIN="src-tauri/target/debug/AztecAccelerator"
[ -x "$BIN" ] || BIN="src-tauri/target/debug/aztec-accelerator"
if [ ! -x "$BIN" ]; then
  echo "ERROR: built binary not found under src-tauri/target/debug" >&2
  exit 1
fi

# ── Launch the app in its own process group, logs captured ──
LOG_DIR="${WEBDRIVER_LOG_DIR:-implementations-plan/presto-visual-rebrand/lessons}"
mkdir -p "$PKG_DIR/../../$LOG_DIR" 2>/dev/null || LOG_DIR="."
APP_LOG="$PKG_DIR/../../$LOG_DIR/webdriver-app.log"
setsid "$BIN" >"$APP_LOG" 2>&1 &
OWNED_PGIDS+=("$!")

# ── Readiness: webdriver port + health endpoint ──
for _ in $(seq 1 60); do
  if ! port_free 4445 && curl -fsS "http://127.0.0.1:59833/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if port_free 4445; then
  echo "ERROR: webdriver port 4445 never became ready; app log: $APP_LOG" >&2
  exit 1
fi

# ── Run the suite ──
bun run test:e2e:webdriver
