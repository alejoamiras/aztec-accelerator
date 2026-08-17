#!/usr/bin/env bash
# B4/F (codex F-review r2): in release-asset mode, verify every downloaded installer in app-artifact/ against
# the GATED SHA-256 manifest (downloaded to manifest/asset-manifest.sha256). This closes the "asset replaced
# for the gate, then restored before finalize" gap — so the bytes the gate tests are provably the manifest's
# bytes (and finalize proves published == manifest). Called by the release-asset legs of _e2e-packaged.yml.
set -euo pipefail

MANIFEST="manifest/asset-manifest.sha256"
test -f "$MANIFEST" || { echo "::error::gated manifest not found at $MANIFEST"; exit 1; }

shopt -s nullglob
cd app-artifact
files=(*)
if [ ${#files[@]} -eq 0 ]; then
  echo "::error::no downloaded installer in app-artifact to verify"
  exit 1
fi
for f in "${files[@]}"; do
  line="$(grep -F "  ${f}" "../$MANIFEST" || true)"
  if [ -z "$line" ]; then
    echo "::error::${f} is not in the gated manifest — refusing (an unexpected asset)"
    exit 1
  fi
  echo "$line" | sha256sum -c -
done
echo "All release-asset installers verified against the gated manifest"
