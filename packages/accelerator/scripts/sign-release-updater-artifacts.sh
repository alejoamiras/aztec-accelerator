#!/usr/bin/env bash
# Production updater signer. Run only in the release-signing environment after dependencies and
# the update-manifest verifier have been prepared. The production private key must be scoped to
# this process; this script never builds, installs, launches, or smoke-tests an application.
set -euo pipefail

ARTIFACTS_ROOT="${1:?usage: sign-release-updater-artifacts.sh <artifacts-root> <version> <release-tag> <repo-root> <manifest-tool>}"
VERSION="${2:?missing version}"
RELEASE_TAG="${3:?missing release tag}"
REPO_ROOT="${4:?missing repo root}"
MANIFEST_TOOL="${5:?missing prebuilt update-manifest tool}"
TAURI_CLI="$REPO_ROOT/packages/accelerator/node_modules/.bin/tauri"

: "${TAURI_SIGNING_PRIVATE_KEY:?production updater signing key is required}"
: "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:?production updater signing key password is required}"
[ -x "$TAURI_CLI" ] || {
  echo "::error::pinned local Tauri CLI is missing at $TAURI_CLI" >&2
  exit 1
}

# Keep the credentials in this shell only. Child processes inherit them solely for the nine
# direct, pinned Tauri signer invocations below; find/jq/date and the verifier never receive them.
SIGNING_PRIVATE_KEY="$TAURI_SIGNING_PRIVATE_KEY"
SIGNING_PRIVATE_KEY_PASSWORD="$TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD

find_one() {
  local directory="$1" pattern="$2"
  local -a matches=()
  while IFS= read -r -d '' match; do matches+=("$match"); done \
    < <(find "$directory" -type f -name "$pattern" -print0)
  if [ "${#matches[@]}" -ne 1 ]; then
    echo "::error::expected exactly one $pattern under $directory, found ${#matches[@]}" >&2
    find "$directory" -type f | sort >&2
    exit 1
  fi
  printf '%s' "${matches[0]}"
}

MAC_ARM_DIR="$ARTIFACTS_ROOT/unsigned-accelerator-macos-arm64"
MAC_INTEL_DIR="$ARTIFACTS_ROOT/unsigned-accelerator-macos-x86_64"
LINUX_DIR="$ARTIFACTS_ROOT/unsigned-accelerator-linux-x86_64"
WINDOWS_DIR="$ARTIFACTS_ROOT/unsigned-accelerator-windows-x86_64"

MAC_ARM_PAYLOAD="$(find_one "$MAC_ARM_DIR" '*.app.tar.gz')"
MAC_INTEL_PAYLOAD="$(find_one "$MAC_INTEL_DIR" '*.app.tar.gz')"
LINUX_PAYLOAD="$(find_one "$LINUX_DIR" '*.AppImage')"
WINDOWS_PAYLOAD="$(find_one "$WINDOWS_DIR" '*-setup.nsis.zip')"

PUBKEY="$ARTIFACTS_ROOT/updater-pubkey.b64"
bun -e "console.log(JSON.parse(await Bun.file('$REPO_ROOT/packages/accelerator/src-tauri/tauri.conf.json').text()).plugins.updater.pubkey)" > "$PUBKEY"

sign_payload() {
  local payload="$1"
  rm -f "$payload.sig"
  ( TAURI_SIGNING_PRIVATE_KEY="$SIGNING_PRIVATE_KEY" \
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$SIGNING_PRIVATE_KEY_PASSWORD" \
    "$TAURI_CLI" signer sign "$payload" )
  "$MANIFEST_TOOL" verify-artifact --artifact "$payload" --sig "$payload.sig" --pubkey "$PUBKEY"
}

sign_payload "$MAC_ARM_PAYLOAD"
sign_payload "$MAC_INTEL_PAYLOAD"
sign_payload "$LINUX_PAYLOAD"
sign_payload "$WINDOWS_PAYLOAD"

size_of() { wc -c < "$1" | tr -d ' '; }
sig_of() { tr -d '\r\n' < "$1.sig"; }

PUB_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
BASE_URL="https://github.com/alejoamiras/aztec-accelerator/releases/download/${RELEASE_TAG}"
UNSIGNED_FEED="$ARTIFACTS_ROOT/latest.unsigned.json"

jq -n \
  --arg version "$VERSION" \
  --arg pub_date "$PUB_DATE" \
  --arg darwin_aarch64_sig "$(sig_of "$MAC_ARM_PAYLOAD")" \
  --arg darwin_aarch64_url "${BASE_URL}/Aztec-Accelerator-${VERSION}-macOS-Apple-Silicon.app.tar.gz" \
  --argjson darwin_aarch64_size "$(size_of "$MAC_ARM_PAYLOAD")" \
  --arg darwin_x86_64_sig "$(sig_of "$MAC_INTEL_PAYLOAD")" \
  --arg darwin_x86_64_url "${BASE_URL}/Aztec-Accelerator-${VERSION}-macOS-Intel.app.tar.gz" \
  --argjson darwin_x86_64_size "$(size_of "$MAC_INTEL_PAYLOAD")" \
  --arg linux_x86_64_sig "$(sig_of "$LINUX_PAYLOAD")" \
  --arg linux_x86_64_url "${BASE_URL}/Aztec-Accelerator-${VERSION}-Linux-x86_64.AppImage" \
  --argjson linux_x86_64_size "$(size_of "$LINUX_PAYLOAD")" \
  --arg windows_x86_64_sig "$(sig_of "$WINDOWS_PAYLOAD")" \
  --arg windows_x86_64_url "${BASE_URL}/Aztec-Accelerator-${VERSION}-Windows-x86_64-setup.nsis.zip" \
  --argjson windows_x86_64_size "$(size_of "$WINDOWS_PAYLOAD")" \
  '{
    version: $version,
    notes: ("Aztec Accelerator " + $version),
    pub_date: $pub_date,
    platforms: {
      "darwin-aarch64": {signature: $darwin_aarch64_sig, url: $darwin_aarch64_url, size: $darwin_aarch64_size},
      "darwin-x86_64": {signature: $darwin_x86_64_sig, url: $darwin_x86_64_url, size: $darwin_x86_64_size},
      "linux-x86_64": {signature: $linux_x86_64_sig, url: $linux_x86_64_url, size: $linux_x86_64_size},
      "windows-x86_64": {signature: $windows_x86_64_sig, url: $windows_x86_64_url, size: $windows_x86_64_size}
    }
  }' > "$UNSIGNED_FEED"

sign_feed() {
  local unsigned="$1" signed="$2"
  local envelope="${unsigned%.json}.envelope.json"
  "$MANIFEST_TOOL" envelope --feed "$unsigned" > "$envelope"
  ( TAURI_SIGNING_PRIVATE_KEY="$SIGNING_PRIVATE_KEY" \
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$SIGNING_PRIVATE_KEY_PASSWORD" \
    "$TAURI_CLI" signer sign "$envelope" )
  "$MANIFEST_TOOL" splice --feed "$unsigned" --envelope "$envelope" --sig "$envelope.sig" > "$signed"
  "$MANIFEST_TOOL" verify --feed "$signed" --pubkey "$PUBKEY"
}

sign_feed "$UNSIGNED_FEED" "$ARTIFACTS_ROOT/latest.json"

make_smoke_feed() {
  local platform="$1" payload="$2" directory="$3"
  local unsigned="$directory/smoke-latest.unsigned.json"
  local basename
  basename="$(basename "$payload")"
  jq --arg platform "$platform" --arg url "https://aztec-accelerator.dev/releases/download/$basename" '
    .notes = ("updater smoke " + .version)
    | .platforms = {($platform): .platforms[$platform]}
    | .platforms[$platform].url = $url
  ' "$UNSIGNED_FEED" > "$unsigned"
  sign_feed "$unsigned" "$directory/smoke-latest.json"
  rm -f "$unsigned" "${unsigned%.json}.envelope.json" "${unsigned%.json}.envelope.json.sig"
}

make_smoke_feed darwin-aarch64 "$MAC_ARM_PAYLOAD" "$MAC_ARM_DIR"
make_smoke_feed darwin-x86_64 "$MAC_INTEL_PAYLOAD" "$MAC_INTEL_DIR"
make_smoke_feed linux-x86_64 "$LINUX_PAYLOAD" "$LINUX_DIR"
make_smoke_feed windows-x86_64 "$WINDOWS_PAYLOAD" "$WINDOWS_DIR"

rm -f "$PUBKEY" "$UNSIGNED_FEED" "${UNSIGNED_FEED%.json}.envelope.json" "${UNSIGNED_FEED%.json}.envelope.json.sig"
echo "Production-signed updater payloads and feeds verified for $VERSION"
