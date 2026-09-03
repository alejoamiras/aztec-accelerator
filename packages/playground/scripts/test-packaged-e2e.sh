#!/usr/bin/env bash
# Build the playground against the packed SDK, serve the production bundle on loopback, then run the
# composed packaged-app proof. The release gate must not depend on Vite's development dependency optimizer:
# stale optimized chunks have returned 504s and, on macOS, left Playwright hung before page navigation.
set -euo pipefail

VITE_LOG="/tmp/packaged-e2e-vite.log"
PREVIEW_PID=""

cleanup() {
  if [ -n "${PREVIEW_PID}" ]; then
    kill "${PREVIEW_PID}" 2>/dev/null || true
    wait "${PREVIEW_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

echo "Building the packaged-E2E playground..."
bun run build

echo "Starting the production preview on http://127.0.0.1:5173..."
bun run preview -- --host 127.0.0.1 --port 5173 --strictPort >"${VITE_LOG}" 2>&1 &
PREVIEW_PID=$!

echo "Waiting for the production preview..."
for attempt in $(seq 1 60); do
  if curl -fsS http://127.0.0.1:5173/ >/dev/null 2>&1; then
    echo "Production preview ready (attempt ${attempt})"
    break
  fi
  if ! kill -0 "${PREVIEW_PID}" 2>/dev/null; then
    echo "::error::Production preview exited before becoming ready"
    cat "${VITE_LOG}" 2>/dev/null || true
    exit 1
  fi
  if [ "${attempt}" -eq 60 ]; then
    echo "::error::Production preview not ready after 30s"
    cat "${VITE_LOG}" 2>/dev/null || true
    exit 1
  fi
  sleep 0.5
done

echo "Running the packaged-app proof against the production preview..."
PLAYWRIGHT_EXTERNAL_WEBSERVER=1 bunx playwright test --project=packaged-e2e --workers=1 "$@"
