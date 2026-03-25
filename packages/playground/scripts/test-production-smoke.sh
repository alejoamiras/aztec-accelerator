#!/usr/bin/env bash
# Build the playground, start vite preview, run Playwright production smoke tests.
# Properly captures the Playwright exit code and cleans up the preview server.
set -euo pipefail

echo "Building playground..."
bun run build

echo "Starting vite preview on port 4173..."
npx vite preview --port 4173 &
PREVIEW_PID=$!

# Wait for the server to be ready
sleep 3

echo "Running production smoke tests..."
RESULT=0
bunx playwright test --project=production-smoke || RESULT=$?

echo "Stopping preview server..."
kill "$PREVIEW_PID" 2>/dev/null || true
wait "$PREVIEW_PID" 2>/dev/null || true

exit "$RESULT"
