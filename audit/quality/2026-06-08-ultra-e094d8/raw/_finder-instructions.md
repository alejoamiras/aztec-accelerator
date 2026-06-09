# Quality-audit finder instructions (shared: Claude + Codex)

You are auditing ONE code cluster for QUALITY (maintainability, not correctness or security). Mindset: **future-change cost**. The code currently works; surface what makes it expensive to change.

Find ONLY concrete code smells with a NAMED catalog mapping. For each finding, produce this certificate:

1. **Title** — concise.
2. **Smell** — from Fowler's Refactoring catalog OR a named close analog (explain the mapping). Catalog:
   - Bloaters: Long Method, Large Class, Primitive Obsession, Long Parameter List, Data Clumps
   - OO-abusers / lang-equiv: Switch-on-type, Temporary Field, Refused Bequest, Alternative Classes w/ Different Interfaces
   - Change preventers: Divergent Change, Shotgun Surgery, Parallel Inheritance
   - Dispensables: Duplicate Code, Dead Code, Lazy Class, Data Class, Speculative Generality, Comments-as-deodorant
   - Couplers: Feature Envy, Inappropriate Intimacy, Message Chains, Middle Man
   - Analogs (cite): Cyclic deps, Temporal Coupling, Config Sprawl, Test Brittleness, error-as-control-flow, sync-over-async. For analogs EXPLAIN the mapping ("this is Shotgun Surgery because changing X needs touching N unrelated files").
3. **Maintenance impact** — bucket {architectural | structural | local | cosmetic} PLUS blast radius (how many files/modules) + change frequency (infer from the code's role; flag if hot path).
4. **Concrete evidence** — cite ALL instances with file:line. Duplication → cite every location + the duplicated logic. Feature Envy → name the envied data + where it lives. Dead code → show absence of inbound refs AND confirm no DI/reflective/registration use.
5. **Why it harms future change** — the concrete scenario that gets harder.
6. **Smallest safe refactoring** — name it from the catalog (Extract Method / Move Function / Replace Conditional w/ Polymorphism / Introduce Parameter Object / Extract Class / Inline / etc.).
7. **What disappears** — the duplication/coupling/complexity removed after the refactor.
8. **Instances** — all file:line locations sharing this root cause.

If you cannot name a smell (Fowler or close analog with mapping), mark it a NON-FINDING. No speculation.

## DO NOT FLAG
- Style/formatting (Biome/rustfmt/clippy handle it).
- Naming preferences without a concrete duplication/coupling consequence.
- "Could be cleaner / more idiomatic" without a named smell.
- Speculative future flexibility ("what if you later need X").
- Performance optimizations (separate concern).
- Security or correctness bugs (other focuses — note in passing at most).
- Pre-existing patterns the codebase uses CONSISTENTLY, unless they create measurable duplication/coupling/change-amplification (conventions aren't smells unless they cost something).
- Issues in test/fixture code UNLESS it's production-wired. (Note: in this repo Rust tests are inline `#[cfg(test)]` in the same file as prod code — audit the PROD items; only flag test code for Test Brittleness if it will break on unrelated refactors.)
- Dead-code claims in registration/callback-slot contexts unless you confirm no caller fills the slot.

## Output
Markdown. One `## Finding N — <title>` per smell with the 8-field certificate. Lead with a one-line cluster verdict (how many findings, rough severity spread). Aim for the SMALLEST set that captures real change-cost; ~1–3 strong findings per cluster beats 8 weak ones. Repo root: `/Users/alejoamiras/Projects/aztec-accelerator`.
