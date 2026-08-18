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

# Swap the packed tarball in for the workspace SDK. `bun add --cwd packages/playground "${ABS}"` LOOPS here:
# bun re-resolves the workspace and the tarball collides with the same-named workspace member
# (error: "@alejoamiras/aztec-accelerator@workspace:packages/sdk has a dependency loop"). The SDK resolves via
# a ROOT node_modules symlink (node_modules/@alejoamiras/aztec-accelerator -> ../../packages/sdk); replace that
# symlink IN PLACE with the EXTRACTED packed tarball, so the playground resolves the PUBLISHED dist with no bun
# re-resolution. The SDK's @aztec/* deps still resolve from the hoisted root node_modules (exact-pinned, one
# graph). A failure here aborts the leg — never silently fall back to the workspace SDK (that would defeat the
# packed-SDK gate).
DEST="node_modules/@alejoamiras/aztec-accelerator"
rm -rf "${DEST}"
mkdir -p "${DEST}"
# npm-pack tarballs nest everything under package/; --strip-components=1 drops that prefix.
tar -xzf "${ABS}" -C "${DEST}" --strip-components=1
test -f "${DEST}/package.json" || { echo "::error::tarball extraction produced no ${DEST}/package.json"; exit 1; }
echo "Playground @alejoamiras/aztec-accelerator now resolves to the packed tarball (version below):"
grep -m1 '"version"' "${DEST}/package.json" || true
