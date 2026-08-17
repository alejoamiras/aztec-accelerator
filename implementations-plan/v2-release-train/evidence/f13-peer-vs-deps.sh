#!/usr/bin/env bash
# F13 decision evidence — WHY the SDK ships its `@aztec/*` as exact-pinned `dependencies`, not
# `peerDependencies`. Reproducible, package/version-agnostic (uses tiny `is-number` 6.0.0 vs 7.0.0; the
# npm placement mechanic does not depend on which package). Run: bash f13-peer-vs-deps.sh
#
# Scenario (both cases): a consumer ("host") pins is-number@6.0.0 directly, and ALSO installs a package
# that wants is-number@7.0.0 — once declaring it as a DEPENDENCY, once as a PEER dependency. This mirrors a
# dApp on one @aztec version consuming the SDK pinned to a different exact @aztec version.
#
# Observed (npm 10, default resolver — captured 2026-08-17):
#   DEP  host: `npm install` SUCCEEDS. Tree nests is-number@7.0.0 under the dep pkg beside root's 6.0.0
#              (2 copies). The dependent is bound to its own correct 7.0.0.
#   PEER host: `npm install` FAILS with ERESOLVE ("peer is-number@7.0.0 ... Found: is-number@6.0.0") on the
#              DEFAULT resolver — not only under --strict-peer-deps. Nothing installs.
#
# Conclusion: peers turn a consumer/SDK version skew into a hard `npm install` failure; exact-pinned deps
# let the install succeed with the SDK bound to its own vetted @aztec (a duplicate graph is the only cost,
# and only when versions differ — an exact-match host dedupes to a singleton, which
# scripts/sdk-tarball-consumer.sh gates on the real tarball). The honest trade-off: KEEP DEPS prioritises
# installability + graceful degradation but DEFERS a cross-version incompatibility to the runtime WASM path;
# peers would fail fast at install but block on ANY skew. Owner choice — see the F13 ledger entry.
set -euo pipefail   # -e so a failed mktemp/pack aborts instead of running against an empty $W
export TMPDIR="${TMPDIR:-$HOME/.cache/tmp}/f13-peer-vs-deps"   # real disk, not tmpfs
mkdir -p "$TMPDIR"
W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT

mkdir -p "$W/dep-pkg" "$W/peer-pkg"
printf '%s\n' '{ "name": "dep-pkg", "version": "1.0.0", "dependencies": { "is-number": "7.0.0" } }' > "$W/dep-pkg/package.json"
printf '%s\n' '{ "name": "peer-pkg", "version": "1.0.0", "peerDependencies": { "is-number": "7.0.0" } }' > "$W/peer-pkg/package.json"
DEP_TGZ="$W/$( cd "$W/dep-pkg" && npm pack --pack-destination "$W" 2>/dev/null | tail -1 )"
PEER_TGZ="$W/$( cd "$W/peer-pkg" && npm pack --pack-destination "$W" 2>/dev/null | tail -1 )"

count_is_number() { ( cd "$1" && find node_modules -type d -path '*is-number' 2>/dev/null | wc -l | tr -d ' ' ); }
# The is-number version that <pkg> ($2) actually resolves from host <dir> ($1).
resolved_version() {
  ( cd "$1" && node -e "const p=require('path');const f=require.resolve('is-number/package.json',{paths:[p.join(process.cwd(),'node_modules','$2')]});console.log(require(f).version)" )
}

RESULT=0
for kind in dep peer; do
  host="$W/host-$kind"
  mkdir -p "$host"
  tgz="$DEP_TGZ"
  if [ "$kind" = peer ]; then tgz="$PEER_TGZ"; fi   # not `[ ] &&` — that would trip `set -e` when false
  printf '{ "name": "host-%s", "version": "0.0.0", "private": true, "dependencies": { "%s-pkg": "file:%s", "is-number": "6.0.0" } }\n' \
    "$kind" "$kind" "$tgz" > "$host/package.json"

  echo "=== $kind host (root is-number@6.0.0, ${kind}-pkg wants 7.0.0), default npm ==="
  set +e
  out="$( cd "$host" && npm install --no-audit --no-fund --loglevel=error 2>&1 )"
  rc=$?
  set -e

  if [ "$kind" = dep ]; then
    # Deps MUST install AND nest the dependent's own correct copy (2 copies, dependent resolves 7.0.0).
    if [ "$rc" -ne 0 ]; then echo "  UNEXPECTED: dep install failed (rc=$rc)"; echo "$out" | tail -3; RESULT=1; continue; fi
    cnt="$(count_is_number "$host")"; ver="$(resolved_version "$host" dep-pkg)"
    echo "  install SUCCESS; is-number copies=$cnt; dep-pkg resolves is-number@$ver"
    [ "$cnt" = 2 ] || { echo "  UNEXPECTED: expected 2 copies (root + nested), got $cnt"; RESULT=1; }
    [ "$ver" = "7.0.0" ] || { echo "  UNEXPECTED: dep-pkg resolved is-number@$ver, expected 7.0.0"; RESULT=1; }
  else
    # Peers MUST fail install, and specifically with ERESOLVE (not some unrelated error).
    if [ "$rc" -eq 0 ]; then echo "  UNEXPECTED: peer install succeeded (expected ERESOLVE)"; RESULT=1; continue; fi
    if printf '%s' "$out" | grep -q "ERESOLVE"; then
      echo "  install FAILED with ERESOLVE (as expected)"
    else
      echo "  UNEXPECTED: peer install failed (rc=$rc) but NOT with ERESOLVE:"; printf '%s\n' "$out" | tail -3; RESULT=1
    fi
  fi
done

if [ "$RESULT" = 0 ]; then
  echo "EVIDENCE HOLDS: dep install succeeds (nested dup), peer install ERESOLVE-fails — KEEP DEPS."
else
  echo "EVIDENCE DIVERGED from the recorded outcome — re-examine the F13 decision." >&2
fi
exit "$RESULT"
