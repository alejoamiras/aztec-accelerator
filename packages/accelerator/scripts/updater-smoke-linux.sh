#!/usr/bin/env bash
# Release-time updater smoke test (Linux / AppImage) — ADVISORY.
#
# Linux sibling of updater-smoke.sh (macOS). Proves a user on the previous stable
# (N-1) AppImage can auto-update to the just-built, just-signed build (N) AND the
# result relaunches reporting version N. This is the open question Tauri's
# `v1Compatible` Linux updater raises: the shipped feed serves a RAW `.AppImage`
# (+ `.AppImage.sig`) — does the updater actually apply it in place and re-exec?
# This test answers that on a real runner. Advisory (NOT in `tag.needs`) until a
# proving run; a red here is a SIGNAL about the Linux update path, not a release
# blocker.
#
# How it works (no signing key needed — identical trust model to macOS):
#   - We serve the ALREADY prod-signed N `.AppImage` from a local HTTPS server
#     impersonating aztec-accelerator.dev (hosts entry + a local CA trusted via
#     `update-ca-certificates`). The updater fetches over reqwest, whose Linux
#     TLS (native-tls→OpenSSL OR rustls→rustls-native-certs — NO bundled
#     webpki-roots in our tree) reads the SYSTEM trust store, so the local CA is
#     honored. N-1 (unmodified) downloads N, verifies the `.sig` against its
#     embedded prod minisign pubkey, swaps in place, and relaunches. We poll
#     /health until it reports version == N.
#
# Differences from macOS (updater-smoke.sh):
#   - CA trust   : update-ca-certificates (system store) vs keychain
#   - install    : cp + chmod +x a writable `.AppImage` vs hdiutil/ditto a .app
#   - execution  : AppImage runtime (needs FUSE — set up by the caller workflow)
#                  sets $APPIMAGE, which the Tauri Linux updater replaces in place
#   - no Gatekeeper/quarantine; GNU `sed -i` (no BSD '' arg)
#   - display    : the caller workflow provides Xvfb + dbus + a tray host
#
# Usage:
#   updater-smoke-linux.sh <n-version> <platform-key> <n-artifacts-dir> <n1-appimage> <repo-root>
#     n-version       e.g. 1.0.3-rc.1   (the version being released)
#     platform-key    linux-x86_64
#     n-artifacts-dir dir with N's *.AppImage + *.AppImage.sig
#     n1-appimage     path to the downloaded N-1 .AppImage
#     repo-root       repo root (to locate the feed server script)
set -euo pipefail

N_VERSION="$1"
PLATFORM_KEY="$2"
N_ARTIFACTS_DIR="$3"
N1_APPIMAGE="$4"
REPO_ROOT="$5"

# positive (default): expect the update to apply (/health reports N).
# negative: serve a TAMPERED AppImage and assert the update is REJECTED. Set via
#           UPDATER_SMOKE_MODE. (The macOS gate already proves signature teeth
#           arch-independently; Linux ships positive-only first, but the mode is
#           wired so a negative leg is a one-line matrix add later.)
MODE="${UPDATER_SMOKE_MODE:-positive}"

HEALTH="http://127.0.0.1:59833/health"
HOST="aztec-accelerator.dev"
CONFIG_DIR="$HOME/.aztec-accelerator"
APP_DIR="$HOME/Applications"
APP_BIN="$APP_DIR/aztec-accelerator.AppImage"
# Run-unique CA name (CN + on-disk filename) so cleanup removes only THIS run's
# trust anchor — a fixed name could clobber a concurrent/leftover entry on a
# non-ephemeral self-hosted runner. (Plan security item; the macOS script's
# fixed-name retro-hardening is tracked as a separate follow-up.)
CA_ID="updater-smoke-local-CA-${GITHUB_RUN_ID:-$$}-${GITHUB_RUN_ATTEMPT:-0}"
CA_DEST="/usr/local/share/ca-certificates/${CA_ID}.crt"
WORK="$(mktemp -d)"
SERVE_DIR="$WORK/serve"
mkdir -p "$SERVE_DIR" "$APP_DIR"

FEED_PID=""
APP_PID=""

log() { echo "── $* ──"; }

# shellcheck disable=SC2329  # invoked indirectly via `trap cleanup EXIT`
cleanup() {
  set +e
  [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null
  # Kill the (possibly relaunched) app by its AppImage path ONLY, dot escaped.
  # A broad `pkill -f aztec-accelerator` ALSO matches THIS script's own argv —
  # the repo checkout path contains "aztec-accelerator" — so it SIGTERMs the
  # script itself mid-cleanup (exit 143), turning a real PASS into a spurious
  # failure. (That false failure was observed on the 1.0.3-rc.11 dry-run.)
  pkill -f "aztec-accelerator\.AppImage" 2>/dev/null
  [ -n "$FEED_PID" ] && sudo kill "$FEED_PID" 2>/dev/null
  # best-effort: drop ONLY the exact line we added (anchored + dots escaped, so
  # the host's literal '.' can't match an arbitrary char) — avoids clobbering an
  # unrelated entry on a self-hosted runner.
  host_re="${HOST//./\\.}"
  sudo sed -i "/^127\\.0\\.0\\.1 $host_re\$/d" /etc/hosts 2>/dev/null
  # best-effort: drop the test CA (matters only on non-ephemeral / self-hosted
  # runners; GH-hosted VMs are torn down after the job).
  sudo rm -f "$CA_DEST" 2>/dev/null
  sudo update-ca-certificates --fresh >/dev/null 2>&1
}
trap cleanup EXIT

# ── Locate N's signed updater artifact ──
N_APPIMAGE="$(find "$N_ARTIFACTS_DIR" -name '*.AppImage' | head -1)"
N_SIG_FILE="$(find "$N_ARTIFACTS_DIR" -name '*.AppImage.sig' | head -1)"
[ -n "$N_APPIMAGE" ] || { echo "::error::no *.AppImage in $N_ARTIFACTS_DIR"; exit 1; }
[ -n "$N_SIG_FILE" ] || { echo "::error::no *.AppImage.sig in $N_ARTIFACTS_DIR"; exit 1; }
N_BASENAME="$(basename "$N_APPIMAGE")"
# Genuine N checksum, captured BEFORE the optional negative-mode tamper, so the
# positive path can assert the on-disk swap landed exactly N's bytes.
N_SUM="$(sha256sum "$N_APPIMAGE" | awk '{print $1}')"
cp "$N_APPIMAGE" "$SERVE_DIR/$N_BASENAME"
N_SIG="$(cat "$N_SIG_FILE")"
log "N artifact: $N_BASENAME (sha256=$N_SUM)"

# Negative control: serve the GENUINE signature but a TAMPERED AppImage (append a
# byte). The updater downloads the artifact, then the minisign check over the
# tampered bytes MUST fail against the embedded pubkey — exercising real
# cryptographic verification (not a malformed-base64 parse error). The genuine
# sig is left untouched so the only fault is the artifact↔signature mismatch.
if [ "$MODE" = "negative" ]; then
  printf 'x' >> "$SERVE_DIR/$N_BASENAME"
  log "NEGATIVE mode: serving a TAMPERED AppImage with the genuine signature — expecting REJECTION (no update)"
fi

# ── Local CA + leaf cert (SAN = the prod host) ──
log "generating local CA + leaf (SAN=$HOST)"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$WORK/ca.key" -out "$WORK/ca.pem" \
  -days 2 -subj "/CN=$CA_ID" >/dev/null 2>&1
openssl req -newkey rsa:2048 -nodes -keyout "$WORK/leaf.key" -out "$WORK/leaf.csr" \
  -subj "/CN=$HOST" >/dev/null 2>&1
cat > "$WORK/leaf.ext" <<EXT
subjectAltName=DNS:$HOST
extendedKeyUsage=serverAuth
EXT
openssl x509 -req -in "$WORK/leaf.csr" -CA "$WORK/ca.pem" -CAkey "$WORK/ca.key" \
  -CAcreateserial -out "$WORK/leaf.pem" -days 2 -extfile "$WORK/leaf.ext" >/dev/null 2>&1

# ── Trust the CA (system store) + impersonate the host ──
# update-ca-certificates regenerates /etc/ssl/certs/ca-certificates.crt, which
# BOTH OpenSSL (native-tls) and rustls-native-certs read — so whichever TLS path
# reqwest takes inside tauri-plugin-updater, the local CA is trusted.
log "trusting CA (update-ca-certificates) + adding hosts entry"
sudo cp "$WORK/ca.pem" "$CA_DEST"
sudo update-ca-certificates >/dev/null 2>&1
echo "127.0.0.1 $HOST" | sudo tee -a /etc/hosts >/dev/null

# ── Synthesize latest.json for N ──
jq -n --arg v "$N_VERSION" --arg key "$PLATFORM_KEY" --arg sig "$N_SIG" \
  --arg url "https://$HOST/releases/download/$N_BASENAME" \
  '{version:$v, notes:("updater smoke "+$v), pub_date:"2026-01-01T00:00:00Z",
    platforms: { ($key): { signature:$sig, url:$url } }}' > "$WORK/latest.json"
log "latest.json:"; cat "$WORK/latest.json"

# ── Start the local HTTPS feed on :443 ──
log "starting feed server on :443"
# sudo for :443; the redirect is opened by this (user) shell so feed.log lands in
# the user-owned workdir — intended, hence SC2024 is not a concern here.
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

# ── Install N-1 to a writable path + make it executable ──
# A real user's AppImage lives at a writable path; the Tauri Linux updater
# replaces the file at $APPIMAGE in place, so this MUST be user-writable (it is —
# $HOME/Applications). Run natively (FUSE, provided by the caller workflow) so
# the AppImage runtime sets $APPIMAGE for the in-place swap.
log "installing N-1 → $APP_BIN"
rm -f "$APP_BIN"
cp "$N1_APPIMAGE" "$APP_BIN"
chmod +x "$APP_BIN"
# Baseline on-disk checksum — the positive path asserts this CHANGED after the
# update (proves the in-place swap physically replaced the file, not just that
# /health happens to report N from some other process).
N1_SUM="$(sha256sum "$APP_BIN" | awk '{print $1}')"
log "N-1 on-disk sha256=$N1_SUM"

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
  # AppImage/FUSE failures surface here (e.g. "dlopen(): error loading libfuse")
  # — distinguishes a harness/FUSE problem from a genuine updater rejection.
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "── NOTE: N-1 process exited (crash or FUSE/AppImage launch failure — see app log above) ──"
  fi
}

if [ "$MODE" = "negative" ]; then
  # Teeth check: the tampered AppImage MUST be rejected. /health must NEVER report
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
  if ! grep -q "/releases/download/" "$WORK/feed.log" 2>/dev/null; then
    echo "::error::NEGATIVE inconclusive — the updater never downloaded the artifact (no download/ hit), so signature rejection was not actually exercised."
    dump_logs
    exit 1
  fi
  log "SUCCESS (negative) — updater downloaded the tampered artifact and refused to update to $N_VERSION"
  dump_logs
  exit 0
fi

# ── Positive: poll /health until version == N (N-1 also answers /health 'ok'
#    with its OWN version, so only version==N counts) ──
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
    # In-place-swap proof: the on-disk AppImage must have CHANGED from N-1. A
    # version flip with an unchanged file would mean Tauri reported N without
    # actually replacing $APPIMAGE (a non-in-place path we'd want to know about).
    POST_SUM="$(sha256sum "$APP_BIN" 2>/dev/null | awk '{print $1}')"
    if [ "$POST_SUM" = "$N1_SUM" ]; then
      echo "::error::/health reports $N_VERSION and the feed was hit, but the on-disk AppImage ($APP_BIN) is UNCHANGED (sha256 still $N1_SUM) — the in-place swap did not happen."
      dump_logs
      exit 1
    fi
    if [ "$POST_SUM" = "$N_SUM" ]; then
      log "in-place swap confirmed — $APP_BIN now matches the served N artifact (sha256=$N_SUM)"
    else
      log "NOTE: post-update sha256=$POST_SUM changed from N-1 but differs from the served N ($N_SUM) — swapped via a re-pack path; /health==N + download hit still confirm the update applied"
    fi
    log "SUCCESS — updated to $GOT via the local feed (artifact downloaded + in-place swap)"
    exit 0
  fi
  sleep 2
done

echo "::error::updater smoke failed — /health never reported version $N_VERSION (advisory: this may indicate Tauri's Linux updater does not apply a raw .AppImage — see app log for FUSE vs updater-reject signal)"
dump_logs
exit 1
