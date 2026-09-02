#!/usr/bin/env bash
# Release-time updater smoke test (macOS).
#
# Proves a user on the previous stable (N-1) can auto-update to the just-built,
# just-signed build (N) AND the result relaunches successfully — the exact
# failure class that shipped in 1.0.1 (amfid hang after the in-place .app swap),
# which fresh-install smoke does not catch.
#
# How it works (no signing key needed):
#   - We serve the ALREADY prod-signed N artifact from a local HTTPS server
#     impersonating aztec-accelerator.dev (hosts entry + a trusted local CA).
#   - N-1 (unmodified, as shipped) fetches its hardcoded endpoint, downloads N,
#     verifies the .sig against its embedded prod pubkey, swaps in place, and
#     restarts. We then poll /health until it reports version == N.
#
# Usage:
#   updater-smoke.sh <n-version> <platform-key> <n-artifacts-dir> <n1-dmg> <repo-root>
#     n-version       e.g. 1.0.3-rc.1   (the version being released)
#     platform-key    darwin-aarch64 | darwin-x86_64
#     n-artifacts-dir dir with N's *.app.tar.gz + *.app.tar.gz.sig
#     n1-dmg          path to the downloaded N-1 .dmg
#     repo-root       repo root (to locate the feed server script)
#
# n-artifacts-dir also contains smoke-latest.json, pre-signed by the isolated
# release signer. This smoke never receives or invokes the production key.
set -euo pipefail

N_VERSION="$1"
PLATFORM_KEY="$2"
N_ARTIFACTS_DIR="$3"
N1_DMG="$4"
REPO_ROOT="$5"

# positive (default): expect the update to apply (/health reports N).
# negative: serve a TAMPERED tarball and assert the update is REJECTED — proves
#           the gate has teeth (a green positive run alone is consistent with a
#           test that can never fail). Set via UPDATER_SMOKE_MODE.
MODE="${UPDATER_SMOKE_MODE:-positive}"

APP="/Applications/Aztec Accelerator.app"
# APP_BIN is resolved AFTER the N-1 install below — resolving here would probe a not-yet-installed
# (or stale leftover) .app, and `set -euo pipefail` turns the failed find into an instant death on
# a clean runner.
HEALTH="http://127.0.0.1:59833/health"
HOST="aztec-accelerator.dev"
CONFIG_DIR="$HOME/.aztec-accelerator"
WORK="$(mktemp -d)"
SERVE_DIR="$WORK/serve"
mkdir -p "$SERVE_DIR"

FEED_PID=""
APP_PID=""

log() { echo "── $* ──"; }

# shellcheck disable=SC2317,SC2329  # invoked indirectly via `trap cleanup EXIT`; SC2317 is only
# emitted by shellcheck < 0.10 (the CI runner's), which doesn't trace trap targets — every command
# in the body reads as unreachable there. Harmless on newer shellcheck, so pin both codes.
cleanup() {
  set +e
  [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null
  pkill -f "Aztec Accelerator.app" 2>/dev/null
  [ -n "$FEED_PID" ] && sudo kill "$FEED_PID" 2>/dev/null
  hdiutil detach "/Volumes/AztecAccelerator-N1" 2>/dev/null
  # best-effort: drop ONLY the exact line we added (anchored + dots escaped, so
  # the host's literal '.' can't match an arbitrary char), not any line
  # mentioning the host — avoids clobbering an unrelated entry on self-hosted.
  host_re="${HOST//./\\.}"
  sudo sed -i '' "/^127\\.0\\.0\\.1 $host_re\$/d" /etc/hosts 2>/dev/null
  # best-effort: remove the test CA we trusted (matters only on non-ephemeral /
  # self-hosted runners; GH-hosted VMs are torn down after the job).
  sudo security delete-certificate -c "updater-smoke-local-CA" \
    /Library/Keychains/System.keychain 2>/dev/null
}
trap cleanup EXIT

# ── Locate N's signed updater artifact ──
N_TARBALL="$(find "$N_ARTIFACTS_DIR" -name '*.app.tar.gz' | head -1)"
N_SIG_FILE="$(find "$N_ARTIFACTS_DIR" -name '*.app.tar.gz.sig' | head -1)"
[ -n "$N_TARBALL" ] || { echo "::error::no *.app.tar.gz in $N_ARTIFACTS_DIR"; exit 1; }
[ -n "$N_SIG_FILE" ] || { echo "::error::no *.app.tar.gz.sig in $N_ARTIFACTS_DIR"; exit 1; }
N_BASENAME="$(basename "$N_TARBALL")"
cp "$N_TARBALL" "$SERVE_DIR/$N_BASENAME"
N_SIG="$(cat "$N_SIG_FILE")"
# Byte size of the GENUINE artifact (before any negative-mode tamper) — bound into the signed
# manifest (F-004 Layer A + SEC-03). BSD/GNU `wc -c` both need the whitespace trimmed.
N_SIZE="$(wc -c < "$N_TARBALL" | tr -d ' ')"
log "N artifact: $N_BASENAME ($N_SIZE bytes)"

# Negative control: serve the GENUINE signature but a TAMPERED tarball (append a
# byte). The updater downloads the artifact, then the minisign check over the
# tampered bytes MUST fail against the embedded pubkey — exercising real
# cryptographic verification (not a malformed-base64 parse error, which a
# corrupted .sig would hit before any verification). The genuine sig is left
# untouched so the only thing wrong is the artifact↔signature mismatch.
if [ "$MODE" = "negative" ]; then
  printf 'x' >> "$SERVE_DIR/$N_BASENAME"
  log "NEGATIVE mode: serving a TAMPERED tarball with the genuine signature — expecting REJECTION (no update)"
fi

# ── Local CA + leaf cert (SAN = the prod host) ──
log "generating local CA + leaf (SAN=$HOST)"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/ca.key" -out "$WORK/ca.pem" \
  -days 2 -subj "/CN=updater-smoke-local-CA" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$WORK/leaf.key" -out "$WORK/leaf.csr" \
  -subj "/CN=$HOST" >/dev/null 2>&1
cat > "$WORK/leaf.ext" <<EXT
subjectAltName=DNS:$HOST
extendedKeyUsage=serverAuth
EXT
openssl x509 -req -in "$WORK/leaf.csr" -CA "$WORK/ca.pem" -CAkey "$WORK/ca.key" \
  -CAcreateserial -out "$WORK/leaf.pem" -days 2 -extfile "$WORK/leaf.ext" >/dev/null 2>&1

# ── Trust the CA (System keychain) + impersonate the host ──
log "trusting CA + adding hosts entry"
sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "$WORK/ca.pem"
echo "127.0.0.1 $HOST" | sudo tee -a /etc/hosts >/dev/null

# ── Stage the pre-signed latest.json for N (F-004 Layer A) ──
SIGNED_FEED="$(find "$N_ARTIFACTS_DIR" -name 'smoke-latest.json' | head -1)"
[ -n "$SIGNED_FEED" ] || { echo "::error::no pre-signed smoke-latest.json in $N_ARTIFACTS_DIR"; exit 1; }
jq -e --arg v "$N_VERSION" --arg key "$PLATFORM_KEY" --arg sig "$N_SIG" \
  --arg url "https://$HOST/releases/download/$N_BASENAME" --argjson size "$N_SIZE" '
    .version == $v
    and (.platforms | keys == [$key])
    and .platforms[$key].signature == $sig
    and .platforms[$key].url == $url
    and .platforms[$key].size == $size
    and (.manifest | type == "string" and length > 0)
    and (.manifest_sig | type == "string" and length > 0)
  ' "$SIGNED_FEED" >/dev/null || {
    echo "::error::pre-signed smoke feed does not bind the exact N artifact"; exit 1;
  }
cp "$SIGNED_FEED" "$WORK/latest.json"
log "latest.json:"; cat "$WORK/latest.json"

# ── Start the local HTTPS feed on :443 ──
log "starting feed server on :443"
# sudo for :443; the redirect is opened by this (user) shell so feed.log lands
# in the user-owned workdir — intended, hence SC2024 is not a concern here.
# shellcheck disable=SC2024
sudo "$(command -v bun)" "$REPO_ROOT/packages/accelerator/scripts/updater-feed-server.ts" \
  --cert "$WORK/leaf.pem" --key "$WORK/leaf.key" \
  --latest-json "$WORK/latest.json" --serve-dir "$SERVE_DIR" > "$WORK/feed.log" 2>&1 &
FEED_PID=$!
for _ in $(seq 1 20); do
  curl -sf "https://$HOST/releases/latest.json" >/dev/null 2>&1 && break
  sleep 0.5
done
curl -sf "https://$HOST/releases/latest.json" >/dev/null || { echo "::error::feed server not reachable"; cat "$WORK/feed.log"; exit 1; }

# ── Install N-1 into /Applications (writable — so the in-place swap + amfid
#    revalidation path that broke 1.0.1 is actually exercised; NOT the
#    read-only DMG-mount pattern the existing smoke job uses) ──
log "installing N-1 from $N1_DMG → /Applications"
rm -rf "$APP"
hdiutil attach "$N1_DMG" -nobrowse -mountpoint /Volumes/AztecAccelerator-N1 >/dev/null
N1_APP="$(find /Volumes/AztecAccelerator-N1 -maxdepth 1 -name '*.app' | head -1)"
ditto "$N1_APP" "$APP"
hdiutil detach /Volumes/AztecAccelerator-N1 >/dev/null
# Approximate the post-Gatekeeper state of a Finder-dragged notarized install
# so N-1 launches headlessly (the update path to N is what we're testing).
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
# N-1 is the real previous release: OLD binary name (aztec-accelerator) until a renamed release
# becomes the fixture, then AztecAccelerator. Resolve whichever THIS install ships — fail if neither.
APP_BIN=$(find "$APP/Contents/MacOS" -maxdepth 1 -type f \( -name AztecAccelerator -o -name aztec-accelerator \) | head -1 || true)
[ -n "$APP_BIN" ] || { echo "FAIL: no main binary in $APP/Contents/MacOS"; exit 1; }

# ── Pre-seed auto-update so N-1 updates without UI ──
mkdir -p "$CONFIG_DIR"
echo '{"config_version":1,"safari_support":false,"approved_origins":[],"speed":"full","auto_update":true}' > "$CONFIG_DIR/config.json"

# ── Launch N-1; it should auto-update to N and relaunch ──
log "launching N-1 (expecting auto-update → N)"
"$APP_BIN" > "$WORK/app.log" 2>&1 &
APP_PID=$!

dump_logs() {
  echo "── app log ──"; cat "$WORK/app.log" 2>/dev/null || true
  echo "── feed log ──"; cat "$WORK/feed.log" 2>/dev/null || true
  echo "── last /health ──"; curl -s "$HEALTH" 2>/dev/null || true
}

if [ "$MODE" = "negative" ]; then
  # Teeth check: the tampered tarball MUST be rejected. /health must NEVER report
  # N (no swap). If it ever does, signature verification has no teeth.
  log "NEGATIVE: asserting /health never reports $N_VERSION (tampered artifact rejected), 120s"
  for _ in $(seq 1 60); do
    GOT="$(curl -sf "$HEALTH" 2>/dev/null | jq -r '.version // empty' 2>/dev/null || true)"
    if [ "$GOT" = "$N_VERSION" ]; then
      echo "::error::NEGATIVE FAILED — a TAMPERED artifact was ACCEPTED; app updated to $N_VERSION. The updater is not verifying signatures."
      dump_logs
      exit 1
    fi
    sleep 2
  done
  # Rule out a VACUOUS pass: the app must actually have DOWNLOADED the artifact
  # (then rejected it). We assert a /releases/download/ hit — NOT latest.json,
  # which our own readiness probe above curls, so it can't prove the app ran.
  if ! grep -q "/releases/download/" "$WORK/feed.log" 2>/dev/null; then
    echo "::error::NEGATIVE inconclusive — the updater never downloaded the artifact (no download/ hit), so signature rejection was not actually exercised."
    dump_logs
    exit 1
  fi
  log "SUCCESS (negative) — updater downloaded the tampered artifact and refused to update to $N_VERSION"
  dump_logs
  exit 0
fi

# ── Positive: poll /health until version == N (the strict success criterion:
#    N-1 also answers /health 'ok' with its OWN version, so only version==N
#    counts) ──
log "polling $HEALTH for version == $N_VERSION (up to 300s)"
for _ in $(seq 1 150); do
  GOT="$(curl -sf "$HEALTH" 2>/dev/null | jq -r '.version // empty' 2>/dev/null || true)"
  if [ "$GOT" = "$N_VERSION" ]; then
    # Guard against a no-op pass: the version flip must have come from OUR feed.
    # Assert a /releases/download/ hit — the app only requests that when it
    # actually downloads N. (We do NOT assert latest.json: our own readiness
    # probe curls it, so it can't prove the app fetched the feed.)
    if ! grep -q "/releases/download/" "$WORK/feed.log" 2>/dev/null; then
      echo "::error::/health reports $N_VERSION but the feed log has no download/ hit — the update did not flow through our feed."
      dump_logs
      exit 1
    fi
    log "SUCCESS — updated to $GOT via the local feed (artifact downloaded)"
    exit 0
  fi
  sleep 2
done

echo "::error::updater smoke failed — /health never reported version $N_VERSION"
dump_logs
exit 1
