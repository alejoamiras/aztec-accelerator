# SDK as published artifact (agent 5)

1. **[HIGH] npm version isn't the SDK's own semver — it's the Aztec protocol version.**
   _publish-sdk.yml:91-98 sets package.json.version to @aztec/stdlib's version (+-revision.N). An
   SDK-breaking change and a protocol-driven publish share one field. -revision.N fix-forwards are
   prereleases that ^X.Y.Z never picks up (promote-latest.yml:7-9). MIGRATION.md documents a real
   breaking TS change with no version boundary. Decide independent SDK semver before v2.0 — hard to
   retrofit after consumers pin. Design + medium CI rework.
2. **[HIGH] @aztec/* are exact-pinned regular dependencies, not peerDependencies.** package.json:26-34
   (exact "5.0.1"), while README:20 describes them as peers. No peerDependencies field. Host dApp with
   its own @aztec/aztec.js at a different version → duplicated nested install → instanceof/wire-format
   mismatch between host PXE and SDK's prover, invisible to tsc. Cheap now, expensive after lockfiles.
3. **[HIGH] Error-handling docs contradict code.** SKILL.md:150-153 claim HTTP errors fall back to
   WASM; accelerator-prover.ts:382-474 only 403 + network-level fall back — any other HTTPError (400
   bad x-aztec-version, 5xx) hits bare `throw err` typed as ky's internal HTTPError, never re-exported.
   No SDK error class. "Present but broken" DOES surface (arguably right) but undocumented + untyped.
   Fix docs / wrap in documented error type.
4. **[MED] App-version / wire-protocol drift invisible to consumers.** /health returns app semver +
   api_version:1 (server.rs:344), but AcceleratorStatus (types.ts:60-96) surfaces neither. A future
   wire bump degrades every SDK to generic reason:"error", no "update the app" signal. Cheap now.
5. **[MED] Publish is token-based, not Trusted Publisher.** _publish-sdk.yml already does id-token +
   --provenance, latest gated via promote-latest, tag-collision check — but auth is static NPM_TOKEN
   (:121), not npm OIDC Trusted Publisher (my-stack's target state). Published package.json shape is an
   inline node -e (:100-113), not diff-reviewable.

Solid: README/SKILL otherwise thorough; public-contract.test.ts pins docs-vs-barrel at test time;
AcceleratorTransport stays internal; checkAcceleratorStatus never throws; the discriminated-union
redesign is good — only its versioning/rollout is the gap.
