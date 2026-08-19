#!/usr/bin/env bash
# B7: consume the PUBLISHED SDK tarball the way a real dApp does — default `npm install` on a fresh Node —
# and prove two things nothing else in the repo checks:
#   1. the packed `dist` exports/types actually RESOLVE + typecheck (the playground uses `workspace:*`, i.e.
#      the source `exports`, so a broken publish rewrite / missing dist would ship undetected);
#   2. the F13 deps-vs-peers decision: default npm's `@aztec/stdlib` graph for an EXACT-version host is a
#      SINGLETON (exact-pinned deps already deliver "one @aztec graph"), and a CONFLICTING-version host is
#      recorded. Peers are not better here — on a skew they ERESOLVE-fail the install; reproducible evidence
#      lives in implementations-plan/v2-release-train/evidence/f13-peer-vs-deps.sh + the F13 ledger entry.
#
#   scripts/sdk-tarball-consumer.sh <absolute-path-to-tarball>
set -euo pipefail

TARBALL="${1:?usage: sdk-tarball-consumer.sh <tarball>}"
[ -f "$TARBALL" ] || { echo "tarball not found: $TARBALL" >&2; exit 2; }
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Count DISTINCT @aztec/stdlib install locations in a consumer dir (1 = singleton graph).
count_stdlib() {
  local dir="$1"
  ( cd "$dir" && find node_modules -type d -path '*@aztec/stdlib' 2>/dev/null | wc -l | tr -d ' ' )
}

make_host() {
  # make_host <dir> <aztec-version>
  local dir="$1" aztec="$2"
  mkdir -p "$dir"
  cat > "$dir/package.json" <<JSON
{
  "name": "host-$aztec",
  "version": "0.0.0",
  "private": true,
  "dependencies": {
    "@alejoamiras/aztec-accelerator": "file:$TARBALL",
    "@aztec/stdlib": "$aztec"
  }
}
JSON
}

# "Exact host" means: the host pins the SAME @aztec/stdlib version the SDK ships with. Derive it
# from the TARBALL UNDER TEST — the artifact's own manifest — never a hardcode (which silently
# manufactures the very skew this gate exists to catch after every @aztec bump; found the hard way
# on 5.0.1 → 5.2.0) and never the workspace manifest (which skews whenever the script is pointed
# at a previously-built tarball). Fails fast, with the reason, if the pin is absent — e.g. if the
# F13 deps-vs-peers decision is ever revisited and @aztec/stdlib leaves `dependencies`.
# shellcheck disable=SC2016  # single quotes are deliberate: the node program must not shell-expand
AZTEC_PIN="$(tar -xzOf "$TARBALL" package/package.json | node -e '
  const manifest = JSON.parse(require("fs").readFileSync(0, "utf8"));
  const pin = (manifest.dependencies ?? {})["@aztec/stdlib"];
  if (!pin) {
    console.error("FATAL: tarball manifest has no dependencies[\"@aztec/stdlib\"] — the exact-host pin cannot be derived");
    process.exit(1);
  }
  // F13 invariant: the SDK ships EXACT pins. A range/alias here (^5.2.0, npm:...) could still
  // resolve to a singleton while silently weakening the exact-pin contract this gate proves.
  // Canonical semver.org expression (no ranges; rejects empty identifiers and leading zeros —
  // npm treats malformed specs like "5.2.0-alpha..x" as mutable TAGS, the opposite of a pin).
  const EXACT_SEMVER = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;
  if (!EXACT_SEMVER.test(pin)) {
    console.error(`FATAL: tarball pins @aztec/stdlib as "${pin}" — not an exact semver; the F13 exact-pin invariant is broken`);
    process.exit(1);
  }
  console.log(pin);
')"

echo "=== exact host ($AZTEC_PIN): the decisive F13 case + tarball resolution ==="
EXACT="$WORK/exact-host"
make_host "$EXACT" "$AZTEC_PIN"
cat > "$EXACT/tsconfig.json" <<'JSON'
{
  "compilerOptions": {
    "module": "nodenext",
    "moduleResolution": "nodenext",
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true,
    "types": []
  },
  "files": ["index.ts"]
}
JSON
# Exercise the FULL published surface — runtime values + types — so a broken barrel/exports/types fails.
cat > "$EXACT/index.ts" <<'TS'
import {
  AcceleratorProver,
  AcceleratorHttpError,
  ACCELERATOR_API_VERSION,
} from "@alejoamiras/aztec-accelerator";
import type { AcceleratorStatus, AcceleratorPhase } from "@alejoamiras/aztec-accelerator";

const _prover: typeof AcceleratorProver = AcceleratorProver;
const _err: typeof AcceleratorHttpError = AcceleratorHttpError;
const _api: number = ACCELERATOR_API_VERSION;
const _phase: AcceleratorPhase = "version-mismatch";
function _use(s: AcceleratorStatus): boolean {
  return s.available && (s.appVersion !== undefined || _api > 0) && _phase.length > 0 && !!_prover && !!_err;
}
void _use;
TS

( cd "$EXACT" && npm install --no-audit --no-fund --loglevel=error )
echo "--- npm ls @aztec/stdlib (exact host) ---"
( cd "$EXACT" && npm ls @aztec/stdlib || true )
EXACT_COUNT="$(count_stdlib "$EXACT")"
echo "exact host @aztec/stdlib install locations: $EXACT_COUNT"

echo "--- typecheck the consumer against the PACKED dist (resolves the 'types' condition) ---"
# `--package=` is required: `typescript` ships both `tsc` and `tsserver`, so `npx typescript` cannot pick a
# binary ("could not determine executable to run"). (codex B7 #1)
( cd "$EXACT" && npx --yes --package=typescript@5.9 tsc --noEmit -p tsconfig.json )

echo "--- RUNTIME import: resolve + load the packed dist 'default' export (types-check can't — codex #2) ---"
cat > "$EXACT/runtime-check.mjs" <<'MJS'
import { AcceleratorProver, AcceleratorHttpError, ACCELERATOR_API_VERSION } from "@alejoamiras/aztec-accelerator";
if (typeof AcceleratorProver !== "function") throw new Error("AcceleratorProver missing from dist");
if (typeof AcceleratorHttpError !== "function") throw new Error("AcceleratorHttpError missing from dist");
if (typeof ACCELERATOR_API_VERSION !== "number") throw new Error("ACCELERATOR_API_VERSION missing from dist");
// The typed error must actually be `instanceof Error` (extends Error), or `catch` narrowing breaks.
if (!(new AcceleratorHttpError(400, "invalid_version") instanceof Error)) throw new Error("AcceleratorHttpError is not an Error");
console.log("runtime import OK: dist exports resolve and load");
MJS
( cd "$EXACT" && node runtime-check.mjs )

echo "=== conflicting host (5.0.0): recorded for the F13 ledger (informational) ==="
CONFLICT="$WORK/conflict-host"
make_host "$CONFLICT" "5.0.0"
( cd "$CONFLICT" && npm install --no-audit --no-fund --loglevel=error ) || echo "conflict host install returned non-zero (ERESOLVE?) — recorded"
echo "--- npm ls @aztec/stdlib (conflict host) ---"
( cd "$CONFLICT" && npm ls @aztec/stdlib || true )
echo "conflict host @aztec/stdlib install locations: $(count_stdlib "$CONFLICT")"

# The decisive gate: the exact host — the supported case — MUST resolve to a single @aztec/stdlib. The
# conflict host is diagnostic only: default npm nests a duplicate here (deps degrade gracefully), whereas
# peers would ERESOLVE-fail the install — see the F13 ledger entry + evidence/f13-peer-vs-deps.sh.
if [ "$EXACT_COUNT" != "1" ]; then
  echo "::error::exact-host resolved $EXACT_COUNT copies of @aztec/stdlib; expected a singleton graph" >&2
  exit 1
fi
echo "OK: packed tarball resolves + typechecks; exact-host @aztec graph is a singleton"
