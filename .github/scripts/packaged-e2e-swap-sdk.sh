#!/usr/bin/env bash
# B4 packaged-E2E: point the playground at the PACKED SDK tarball instead of the workspace source, so the
# composed proof exercises the exact artifact that would publish (not the workspace tree).
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
# (error: "@alejoamiras/aztec-accelerator@workspace:packages/sdk has a dependency loop"). Under the isolated
# linker the playground resolves the SDK through its OWN node_modules
# (packages/playground/node_modules/@alejoamiras/aztec-accelerator -> ../../../sdk — there is no hoisted root
# copy); replace that symlink IN PLACE with the EXTRACTED packed tarball, so the playground resolves the
# packed code with no bun re-resolution. The tarball ships no node_modules, so link the workspace SDK's own
# node_modules into it — the packed code resolves its deps (@aztec/*, ...) through the exact pinned graph
# the workspace SDK uses (one graph). A failure here aborts the leg — never silently fall back to the
# workspace SDK (that would defeat the packed-SDK gate).
REPO_ROOT="$(pwd)"
DEST="packages/playground/node_modules/@alejoamiras/aztec-accelerator"
rm -rf "${DEST}"
mkdir -p "${DEST}"
# npm-pack tarballs nest everything under package/; --strip-components=1 drops that prefix.
tar -xzf "${ABS}" -C "${DEST}" --strip-components=1
test -f "${DEST}/package.json" || {
  echo "::error::tarball extraction produced no ${DEST}/package.json"
  exit 1
}
ln -s "${REPO_ROOT}/packages/sdk/node_modules" "${DEST}/node_modules"

# Prove the swap FROM THE CONSUMER: playground resolution must land inside the swapped dir. This is the
# guard against a layout change quietly re-routing resolution back to the workspace source — the exact
# silent fallback this gate exists to prevent (it happened once: a linker change removed the hoisted root
# symlink an earlier version of this script swapped, and the leg kept passing against the workspace SDK).
RESOLVED="$(bun -e "console.log(Bun.resolveSync('@alejoamiras/aztec-accelerator', '${REPO_ROOT}/packages/playground'))")"
case "${RESOLVED}" in
  "${REPO_ROOT}/${DEST}"/*)
    echo "Swap verified from the consumer: ${RESOLVED}"
    ;;
  *)
    echo "::error::packed-SDK swap ineffective — playground resolves ${RESOLVED} (expected inside ${DEST})"
    exit 1
    ;;
esac
# And the packed code must reach its own deps through the linked graph.
for dep in @logtape/logtape @aztec/bb-prover; do
  bun -e "Bun.resolveSync('${dep}', '${REPO_ROOT}/${DEST}')" || {
    echo "::error::packed SDK cannot resolve its dependency '${dep}' from ${DEST}"
    exit 1
  }
done

echo "Playground @alejoamiras/aztec-accelerator now resolves to the packed tarball (version below):"
grep -m1 '"version"' "${DEST}/package.json" || true
