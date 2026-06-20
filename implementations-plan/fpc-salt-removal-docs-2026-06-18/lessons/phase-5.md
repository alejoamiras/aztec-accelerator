# Phase 5 — Re-deploy playground to v5 + smoke (2026-06-20)

Dispatched `publish-testnet.yml --ref main -f skip_sdk_publish=true` (run `27845607437`) from the merged salt-less `main`. Pipeline: changes ✓ → e2e ✓ → **deploy-app** ✓ (build → S3 sync → CloudFront invalidate) → publish-sdk **skipped** (skip_sdk_publish).

**Gate — PASS:**
- Live bundle (`/assets/index-*.js`): `SPONSORED_FPC_SALT` = **0 occurrences**; baked `process.env` = `{AZTEC_NODE_URL:"https://v5.testnet.rpc.aztec-labs.com", VITE_AZTEC_SDK_VERSION:"5.0.0-rc.1"}` (no salt key). v5 host ×2, `5.0.0-rc.1` ×6. HTML + bundle HTTP 200.
- FPC precondition: `node_getContract(0x261366b3…7880)` on `v5.testnet.rpc.aztec-labs.com` → **DEPLOYED**.
- **Live browser smoke: PASSED** (user-confirmed "playground worked" — a deploy/transfer proved + mined paying via the canonical salt=0 FPC).

## Gotchas
- **`grep | head && echo` is a false-positive trap.** `grep -oE PAT file | head -1 && echo "FOUND"` fires the `&&` branch *regardless* of whether grep matched, because `head` exits 0. Three "⚠️ still baked" scares were all this bug, not real matches. Use `grep -c` / `grep -oF … | wc -l` (proper exit + count) for presence/absence assertions.
- **`@aztec/constants` exports a `SPONSORED_FPC_SALT` symbol**, so the string *could* legitimately appear in a bundle from the dep — but minification tree-shakes the name (value 0 inlined), so the live bundle has it 0×. The decisive salt-less check is the **baked `process.env` object**, not a raw string grep.
- **Standalone `bun script.ts` for an FPC check mis-resolved** to a transitive `@aztec/stdlib@4.3.1` subpath (`@aztec/stdlib/logs` not found). Bypassed it with a **direct node JSON-RPC curl** (`node_getContract`) — robust, no JS-SDK resolution needed.

LESSONS_FILE=implementations-plan/fpc-salt-removal-docs-2026-06-18/lessons/phase-5.md
