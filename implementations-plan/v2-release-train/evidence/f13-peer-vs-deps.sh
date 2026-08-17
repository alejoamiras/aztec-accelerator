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
# scripts/sdk-tarball-consumer.sh gates on the real tarball). For a drop-in "just works, falls back to WASM"
# SDK, breaking install is strictly worse. Hence KEEP DEPS.
set -uo pipefail
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

RESULT=0
for kind in dep peer; do
  host="$W/host-$kind"
  mkdir -p "$host"
  tgz="$DEP_TGZ"; [ "$kind" = peer ] && tgz="$PEER_TGZ"
  printf '{ "name": "host-%s", "version": "0.0.0", "private": true, "dependencies": { "%s-pkg": "file:%s", "is-number": "6.0.0" } }\n' \
    "$kind" "$kind" "$tgz" > "$host/package.json"

  echo "=== $kind host (root is-number@6.0.0, ${kind}-pkg wants 7.0.0), default npm ==="
  if ( cd "$host" && npm install --no-audit --no-fund --loglevel=error >/dev/null 2>&1 ); then
    echo "  install: SUCCESS; is-number copies=$(count_is_number "$host")"
    [ "$kind" = dep ] || { echo "  UNEXPECTED: peer host install succeeded"; RESULT=1; }
  else
    echo "  install: FAILED (ERESOLVE)"
    [ "$kind" = peer ] || { echo "  UNEXPECTED: dep host install failed"; RESULT=1; }
  fi
done

if [ "$RESULT" = 0 ]; then
  echo "EVIDENCE HOLDS: dep install succeeds (nested dup), peer install ERESOLVE-fails — KEEP DEPS."
else
  echo "EVIDENCE DIVERGED from the recorded outcome — re-examine the F13 decision." >&2
fi
exit "$RESULT"
