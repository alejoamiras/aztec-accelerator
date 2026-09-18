# Recon: B7 — SDK as published artifact

Agent: sonnet Explore, 2026-08-16, tree @ 0c351bc.

## package.json / build / tests

- version "0.0.0" placeholder (:3); exports "./src/index.ts" (:11) — source, rewritten at publish.
- files ["src","dist",".claude"] (:12-16) — tarball ships BOTH src and dist; `.claude` = consumer skill.
- **MIGRATION.md is NOT in `files`** and is not an npm always-included root file → probably NOT in the
  tarball at all. Verify with `npm pack --dry-run` during B7.
- deps: @aztec/{bb-prover,foundation,noir-acvm_js,noir-noirc_abi,stdlib} EXACT "5.0.1"; ky ^2.0.2,
  @logtape ^2.0, ms ^2.1.3. devDeps: more @aztec exact 5.0.1. **NO peerDependencies field** despite
  README:20 + SKILL.md:14 claiming peer-dep semantics. publishConfig.access=public only.
- tsconfig: rootDir src, outDir dist, excludes *.test.ts from BUILD but `files:["src"]` ships test
  files in src/ anyway.
- Tests: bun:test; public-contract.test.ts = doc-sync guard (barrel exports, README union/setForceLocal,
  "denied" phase in README+SKILL, MIGRATION mentions AcceleratorProtocol). e2e/ gated by env.
- No CHANGELOG. No error-types file.

## Publish flow

- _publish-sdk.yml: dispatch (testnet|nightlies only — `latest` structurally unreachable, guard :45-52);
  concurrency group publish-npm shared with promote-latest (never cancels). Node 24 + Bun latest
  (outlier; everywhere else Bun 1.3.14). Version = @aztec/stdlib pin → scripts/get-sdk-publish-version.ts
  (resolvePublishVersion :27-49, tested): stable base → `-revision.N`; prerelease base → dot-append.
- Exports rewrite = inline `node -e` (:100-113) mutating built package.json (main/types/exports→dist)
  — NOT diff-reviewable; candidate to become a committed script.
- Auth: NODE_AUTH_TOKEN = NPM_TOKEN (static; OIDC deferred post-v2 per brief). `npm publish
  --provenance --access public --tag $DIST_TAG --workspaces=false`; id-token:write for Sigstore.
- Tag `@alejoamiras/aztec-accelerator@$VERSION`; collision check via ls-remote (:140-153); gh release
  create `--latest=false` ALWAYS (:166-173). **No --notes-file, no MIGRATION attachment** — the B7 gap.
- promote-latest.yml: dispatch-only; regex-validates version; verifies already-on-npm; `npm dist-tag
  add ... latest` — the ONLY path moving `latest`; doubles as rollback lever (comments :1-9). Policy
  documented ONLY in those comments — zero mentions in docs/RELEASE_RUNBOOK.md.
- Chain: publish-testnet.yml (assert-main :32-42) → _e2e.yml (build_accelerator:true) → _publish-sdk
  (testnet) → deploy-app.

## Error paths (accelerator-prover.ts #proveRemote, catch block :399-536)

Falls back to WASM: 403 (→"denied" phase, :400-412), 503 only (:424-430, deliberate not-blanket-5xx),
non-HTTPError network failures (:433-534, with HTTPS→HTTP demotion retry — retry path falls back on
ANY failure :515-521), endpoint-generation mismatch, decode-failure-after-2xx (:544-554).
**Escapes RAW (`throw err` :535): every other HTTPError** — 400 invalid_version/invalid_origin,
408 body_read_timeout, 413 payload_too_large, 429 too_many_requests/prove_queue_full,
500 download_failed/prove_failed. Untyped (ky HTTPError, not re-exported). NO SDK error class exists
(zero `extends Error` hits). **No test pins the escape path** (nothing asserts 400/408/413/429/500
propagate).

- 403 CONFLATION: server has FOUR 403 variants (OriginDenied, VersionNotAllowed,
  AuthorizationTimeout, AuthorizationCancelled — ProveError table server.rs:462-567) — SDK collapses
  all into "denied"/WASM. VersionNotAllowed ≠ user denial semantically.
- SKILL.md:145-153 "Accelerator returns HTTP error: falls back to WASM" is FALSE as stated. README
  :159-161 more careful but never documents the escape path.
- Wire contract: text/plain carrying JSON string (json_error server.rs:580-583), pinned by Rust test
  prove_error_responses_stay_text_plain_json_string; invalid_host is the application/json exception.
- transport throws plain Error from config-time validators only; probeHealth failures never escape
  (→ {available:false,"offline"}). AcceleratorTransport internal, not in barrel (index.ts, 9 lines).

## AcceleratorStatus vs /health

- types.ts union at :76-112 (brief's 60-96 anchor DRIFTED). Carries aztec-version info; **no app
  `version`, no `api_version` field**.
- server.rs health() :405-453 (brief's 319-350 DRIFTED): minimal {"status","api_version":1} for
  unapproved cross-origin (SEC-05 tiering :380-403); detailed adds version/aztec_version/
  available_versions/bb_available/https_port.
- SDK #classifyHealth (:254-302) destructures only {aztec_version, available_versions} — version +
  api_version discarded on parse. `api_version:1` magic literal in THREE places: server.rs:411/429,
  core probe.rs:52-53, transport isRecognizedHealthBody :159-163 — no shared negotiation.

## CI slot for tarball-consumer job

- sdk.yml: changes filter → lint/typecheck/unit-tests/e2e → sdk-status aggregate (:118-135; new job
  must join needs list :121 + results string :127).
- Steps: build → npm pack (or bun pm pack) → install tarball into throwaway Node-tsc + Vite fixture
  dirs → typecheck+build each. NO precedent in-repo (grep npm pack → only Tauri sidecar hits).
- Node pinned ONLY in publish workflows (24); no engines fields; no Node matrix precedent anywhere —
  a fresh-Node consumer job introduces Node-version choice to CI for the first time.
- Playground consumes SDK via workspace:* (playground/package.json:22) — validates source API, can
  NEVER catch publish-time regressions (exports rewrite breakage, missing dist, wrong main/types).

## peerDeps decision inputs

- Exact-pin rationale trail: bunfig minimumReleaseAge=604800 affects RESOLUTION only; frozen-lockfile
  CI installs vetted lock. aztec-5.0.0-rc.2 plan + audit-fable document the vetted-once-frozen-forever
  story; exactness is what keeps the next `bun update` from silently re-resolving into the fresh
  window.
- Peer-move blast radius: sdk + playground manifests (accelerator has no @aztec deps). Playground
  already declares own matching @aztec — low risk. Real risk external: peer ^range + host on
  different aztec → nested duplicate stdlib → instanceof/wire mismatch. e2e/workspace consumers
  can't see the difference; only the new tarball job exercises foreign-host resolution.
