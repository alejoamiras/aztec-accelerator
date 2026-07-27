#!/usr/bin/env bash
# Sign a synthesized updater-smoke feed IN PLACE (F-004 Layer A).
#
# A C4+ N-1 build enforces the signed-manifest envelope, so the local smoke feed must carry a
# `manifest` + `manifest_sig` signed with the SAME updater key N-1 embeds (the prod key — the synthetic
# N-1 keeps the committed prod pubkey). This assembles the canonical envelope from the feed, signs it
# with the key in TAURI_SIGNING_PRIVATE_KEY[_PASSWORD], and splices the two fields back in. Shared by
# updater-smoke.sh (macOS) + updater-smoke-linux.sh so the envelope shape has ONE definition.
#
# Encoding contract (kept in lockstep with accelerator_core::update_manifest):
#   - manifest     = base64(envelope.json bytes)
#   - manifest_sig = the .sig file content VERBATIM (tauri writes it as base64(minisign doc))
#
# Usage: sign-smoke-feed.sh <feed.json> <repo-root>
set -euo pipefail

FEED="$1"
REPO_ROOT="$2"
: "${TAURI_SIGNING_PRIVATE_KEY:?TAURI_SIGNING_PRIVATE_KEY is required to sign the smoke feed}"

ENVELOPE="$(dirname "$FEED")/envelope.json"
# Envelope shape MUST match accelerator_core::update_manifest::SignedEnvelope (deny_unknown_fields):
# exactly {schema, version, pub_date, platforms:{key:{url,size,signature}}}. The feed's platform
# entries already carry exactly {signature,url,size}, so projecting them is field-name-exact.
jq '{schema: "aztec-accelerator-update-manifest-v1", version, pub_date, platforms}' "$FEED" > "$ENVELOPE"

# Sign the exact envelope bytes with the updater key (bunx resolves @tauri-apps/cli from the package).
( cd "$REPO_ROOT/packages/accelerator" && bunx tauri signer sign "$ENVELOPE" )

# base64 of the signed bytes; strip newlines (BSD + GNU base64 both wrap by default).
MANIFEST_B64="$(base64 < "$ENVELOPE" | tr -d '\n')"
MANIFEST_SIG="$(cat "$ENVELOPE.sig")" # already base64(minisign doc) — embed verbatim

jq --arg m "$MANIFEST_B64" --arg s "$MANIFEST_SIG" '. + {manifest: $m, manifest_sig: $s}' \
  "$FEED" > "$FEED.signed"
mv "$FEED.signed" "$FEED"
echo "── signed smoke feed: manifest + manifest_sig spliced ──"
