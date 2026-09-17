# Phase 2 lessons — CI installer min-age exemption (2026-08-19)

**Outcome: ✓ green. Commit `2e78bcc`.**

- Change: `setup-aztec/action.yml` Install-Aztec-CLI step — `npm install -g npm@12.0.2` +
  `export npm_config_min_release_age_exclude='@aztec/*'` (alongside the existing
  `min_release_age=7`), cache salt `-minage7` → `-minage7-npm12-aztecexempt`.
- Structural check before validating: the installer's `install_node` no-ops when ambient node ≥
  min version → CI keeps setup-node's Node 24 (and thus the upgraded npm 12); the installer's
  `PATH` prepend exposes only bundled native tools (nargo/forge), never node/npm. The npm leg is
  literally `npm install @aztec/aztec@V @aztec/cli-wallet@V @aztec/bb.js@V --prefix <dir>` with
  ambient npm + env.
- **Validation harness lesson**: v1 tried the installer end-to-end under an overridden `HOME` —
  noirup's installer resolves home via passwd (`~user`), ignoring `$HOME`, so it wrote the real
  `~/.nargo` while the aztec script checked scratch-HOME → 3 retries, fail. NOT a CI-relevant
  failure (CI never overrides HOME) and orthogonal to the npm change (noir leg precedes npm).
  v2 validated the exact npm leg instead, with a causality control.
- v2 results (all under npm 12.0.2): CONTROL (quarantine, no exclude) → `ETARGET` (quarantine
  bites, as in CI today); REAL (with `@aztec/*` exclude) → install resolves; verbatim snappy
  probe → **PASS**.
- **Watch-item (informational, fail-closed criteria met)**: npm 12 blocked 6 packages' install
  scripts (`allowScripts` default; e.g. `msgpackr-extract`'s `node-gyp-build-optional-packages`).
  Snappy loads regardless (prebuilt napi bindings, no script needed); msgpackr falls back to
  pure-JS if its native extract is absent (perf, not correctness). If Phase-3 CI's sandbox boot
  (`aztec start` in `_e2e.yml`) fails on a missing native binding, the remedy is a minimal
  per-package allowlist via plan revision — never blanket enablement.
- `bun run lint:actions`: exit 0.
- Scope note: "installer end-to-end locally" was narrowed to "installer's npm leg + probe,
  locally, with control" for the harness reason above; the true end-to-end runs in CI at Phase 3
  (the sandbox e2e legs), which is the enforcing gate anyway.

LESSONS_FILE=implementations-plan/aztec-5.2.0-2026-08-18/lessons/phase-2.md
