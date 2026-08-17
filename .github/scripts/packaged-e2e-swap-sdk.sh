#!/usr/bin/env bash
# B4 packaged-E2E: point the playground at the PACKED SDK tarball instead of the workspace source, so the
# composed proof exercises the exact artifact that would publish (not the monorepo-hoisted workspace tree).
# Shared by the Linux + macOS legs of `_e2e-packaged.yml` (codex harness review: workspace SDK is not a
# release gate). The tarball's packaging (files set, entry points, dep resolution) is ALSO gated by
# scripts/sdk-tarball-consumer.sh; this leg additionally proves it PROVES.
set -euo pipefail

echo "Building the SDK..."
bun run --cwd packages/sdk build

# `npm pack` prints notices to stderr and the tarball filename to stdout; --silent keeps stdout to just it.
TARBALL="$(cd packages/sdk && npm pack --silent | tail -1)"
ABS="$(cd packages/sdk && pwd)/${TARBALL}"
if [ ! -f "${ABS}" ]; then
  echo "::error::packed SDK tarball not found at ${ABS}"
  exit 1
fi
echo "Packed ${ABS}"

# Install the tarball into the playground, overriding the workspace:* resolution for this run. A failure here
# must abort the leg (never silently fall back to the workspace SDK — that would defeat the packed-SDK gate).
bun add --cwd packages/playground "${ABS}"

echo "Playground @alejoamiras/aztec-accelerator now resolved from the packed tarball:"
grep -m1 "aztec-accelerator" packages/playground/package.json || true
