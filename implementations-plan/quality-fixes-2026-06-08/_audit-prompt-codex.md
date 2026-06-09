You are auditing a CONSOLIDATED implementation plan (blueprint `deep` — combined contradiction-check + adversarial/security/assumption audit). cwd = repo root. Quality-refactor plan for 9 audit findings.

READ: `implementations-plan/quality-fixes-2026-06-08/plan.md` (the consolidated plan + decision ledger), `_brief.md` (fixed decisions + constraints), and the actual source it touches (`core/src/{authorization,config,server,versions}.rs`, `core/src/server/prove.rs`, `src-tauri/src/{main,commands,certs,server}.rs`, `server/src/main.rs`, `sdk/src/lib/accelerator-prover.ts`, `sdk/src/index.ts`). The 3 source planner files `_planner-{main,codex,opus}.md` are available if useful.

Do BOTH jobs:

**A) CONTRADICTION-CHECK** — cross-phase contradictions (a per-finding decision that contradicts the PR ordering / test plan / a hard constraint); rejected alternatives that should have been kept; disputed items silently resolved. SPECIFICALLY stress-test these flagged decisions:
1. **`DesktopCallbacks` (F-01)** — adopted over flat `AppState`. Is it actually safe? Does it break the `AppState: Deref<Target=HeadlessState>` (callbacks are separate fields), force changes to the Tauri command layer, or add churn that risks behavior change? Or is flat-AppState genuinely the safer minimal-diff choice?
2. **F-02 env=fail-fast vs persisted=lenient-drop** — is the two-policy split sound, or inconsistent/surprising? Does fail-fast on `ALLOWED_ORIGINS` risk breaking an operator who set a now-stricter value?
3. **F-02-first-then-F-01 ordering** in PR-1 — any circular/type dependency that makes this impossible?
4. The **`migrate_approved_origins` deletion** — does anything (config resave, dedupe, ordering, another caller) depend on it beyond what the serde helper replaces?

**B) ADVERSARIAL + SECURITY + ASSUMPTION attack** — What breaks in production? What would an attacker target on F-02 origin canonicalization (any vector the security matrix misses — e.g. `blob:`, `null` origin, IPv6 `[::1]`, port `0`, overlong, mixed-case punycode, percent-encoded host)? What are we trusting we shouldn't? Is the SDK truly non-breaking (F-05 additive export + F-06 internal-only — could either alter the published `.d.ts` surface)? Does any "behavior-preserving" refactor (F-03/F-04/F-07/F-08/F-09) secretly change behavior? **ATTACK THE ASSUMPTIONS SECTION**: which Facts are misstated, which Inferences unsafe, which Asks need surfacing — return these under Facts / Inferences / Asks buckets.

OUTPUT: lead with a one-line verdict (`sound` / `issues: …`). Then findings ranked by severity (High/Med/Low), each naming the exact plan section + a concrete fix. Terse + specific, ~600–900 words.
