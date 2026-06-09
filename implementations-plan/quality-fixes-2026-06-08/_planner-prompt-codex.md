You are an independent software architect producing an IMPLEMENTATION PLAN (blueprint `deep`, 1 of 3 parallel planners — main agent + you + an Opus planner draft independently, then get consolidated and audited). Your cwd is the repo root.

READ FIRST (don't trust summaries — read the real source):
- `implementations-plan/quality-fixes-2026-06-08/_brief.md` — the task, the FIXED user decisions, hard constraints, and known sharp edges. Treat the 4 decisions as settled; do NOT relitigate them.
- `audit/quality/2026-06-08-ultra-e094d8/report.md` + `findings/verified.md` — the 9 findings in detail with file:line.
- The actual source each finding cites (e.g. `packages/accelerator/core/src/{server.rs,authorization.rs,config.rs,versions.rs,server/prove.rs}`, `packages/accelerator/src-tauri/src/{main.rs,commands.rs,certs.rs}`, `packages/accelerator/server/src/main.rs`, `packages/sdk/src/lib/accelerator-prover.ts`, `packages/sdk/src/index.ts`).

PRODUCE a complete, independent implementation plan for all 9 findings (F-01…F-09):
1. **Per-finding approach** — the concrete refactoring + the EXACT new type/constructor/module shapes (write the signatures), what moves where, and which call sites update.
2. **The 4 package-coherent PRs** (PR-1 F-01+F-02, PR-2 F-03+F-04, PR-3 F-07+F-08+F-09, PR-4 F-05+F-06): intra-PR step ordering, and any dependency between findings (e.g. does F-02's newtype touch F-01's struct?).
3. **Per-finding TEST PLAN** — which EXISTING tests already cover it; which NEW tests to add (honor the brief's *no-blanket-characterization* steer — new tests only for new seams/behavior); and the **validity argument** (how we are SURE it's correct: compiler, existing WebDriver E2E, new unit test). Call out any refactor that is risky AND uncovered (where a characterization test might be warranted).
4. **Risk + rollback** per PR.
5. **Security & Adversarial Considerations** — especially F-02 origin canonicalization: enumerate what an attacker could try to smuggle past exact-match approval (IDN/punycode, uppercase host, trailing dot, default vs explicit port, scheme http/https/ws, userinfo `user@host`, path/query, embedded `\0`/whitespace) and how `url::Url` + the `CanonicalOrigin` newtype handle each. Does closing the headless `ALLOWED_ORIGINS` bypass change any trust assumption or break the e2e harness?
6. **Assumptions** — Facts (cite file:line) / Inferences (label as unverified) / Asks (decisions to surface to the user).

Be ADVERSARIAL about your OWN plan: where could each "behavior-preserving" refactor silently change behavior? Which "pure move" isn't pure? Attack the F-02 serde migration (existing persisted `approved_origins` must still deserialize), the F-01 non-`Option` change (the headless binary sets `config: None` — that field must stay optional), and the F-04 module split's public-path stability (it's `pub`-consumed by `bb.rs` + server + src-tauri).

~1500–2500 words, concrete. This is one of three independent plans.
