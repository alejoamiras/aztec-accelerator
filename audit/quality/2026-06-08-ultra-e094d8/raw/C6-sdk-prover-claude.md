# Cluster C6 — sdk-prover (`@alejoamiras/aztec-accelerator`) — quality findings (Claude)

**Verdict:** 6 findings. Severity spread: 1 structural-high (README documents a stale public type the compiler no longer produces — a discriminated-union/flat mismatch that will mislead every consumer doing type-driven narrowing), 3 structural-medium (two divergent HTTP stacks; Long Method `#probeAndParseHealth`; error-as-control-flow flattening every health failure to `offline`), 2 local (barrel/module export drift on `AcceleratorProtocol`; undocumented-but-shipped `setForceLocal`). This is the **weighted** cluster (public consumer contract) so the doc-drift items are scored up: a wrong type in the README costs every integrator a debug cycle.

Scope note: the prod surface is a single file (`accelerator-prover.ts`, 494 LOC) plus a 2-line `logger.ts` and an 8-line barrel. Doc files (`README.md`, `MIGRATION.md`, `SKILL.md`) are the contract mirror and are audited for drift against the actual exports.

---

## Finding 1 — README documents the OBSOLETE flat `AcceleratorStatus`; actual export is a 4-arm discriminated union

**Smell:** Doc-contract drift (named analog of Divergent Change / Comments-as-deodorant applied to the published API contract). Mapping: the type the SDK *exports* and the type the SDK *documents as its API reference* have diverged. The README's `AcceleratorStatus` is character-for-character the pre-Q12 "Before" shape that `MIGRATION.md` explicitly labels obsolete — so the project's own two docs contradict each other, and the canonical one (README, the npm landing page) shows the dead shape.

**Maintenance impact:** structural; blast radius = every external consumer of `checkAcceleratorStatus()` (the README is the npm-rendered API reference — highest-visibility doc in the package). Change frequency: the status union is the hottest part of the public contract (Q12 already broke it once); doc lag here re-bites on every future status change.

**Concrete evidence:**
- Documented (stale, flat — all fields optional, no `reason` discriminant): `packages/sdk/README.md:92-101`
  ```ts
  interface AcceleratorStatus {
    available: boolean;
    needsDownload: boolean;        // claimed present unconditionally
    acceleratorVersion?: string;
    availableVersions?: string[];
    sdkAztecVersion?: string;
    protocol?: "http" | "https";   // claimed optional
  }
  ```
- Actual export (discriminated union on `available`, with a `reason` discriminant on the false arm and `protocol` *required* on the available/error/mismatch arms): `packages/sdk/src/lib/accelerator-prover.ts:56-92`.
- `MIGRATION.md:9-20` reproduces the README's exact block under the heading **"### Before"** and `MIGRATION.md:22-43` shows the union as **"### After"** — i.e. the migration guide already declares the README's shape dead.

**Why it harms future change:** A consumer reading the README writes `if (status.available && !status.needsDownload)` or reads `status.protocol` unconditionally — both of which the real union rejects (`needsDownload`/`availableVersions` don't exist on the unavailable arms; there is no `reason` field in the doc at all, so the consumer never learns to switch on it). The doc actively teaches the anti-pattern the Q12 refactor was designed to prevent. Every status-shape change must now be made in *three* places (type, README, MIGRATION) or the drift widens.

**Smallest safe refactoring:** Replace the README `AcceleratorStatus` block (`README.md:92-101`) with the union from `MIGRATION.md:22-43` (or a `// see MIGRATION.md` pointer), and add the `reason` field to the prose. Single source of truth: generate the API-reference type block from the `.ts` source, or link to MIGRATION instead of re-stating.

**What disappears:** The README/MIGRATION self-contradiction and the false flat-interface that mis-teaches narrowing. Consumers get the same shape the compiler enforces.

**Instances:** `README.md:92-101`; contradicted-by `accelerator-prover.ts:56-92` and `MIGRATION.md:9-43`.

---

## Finding 2 — Two HTTP stacks with divergent timeout/retry/error semantics (`fetch` for `/health`, `ky` for `/prove`)

**Smell:** Duplicate Code + Divergent Change + Alternative Classes with Different Interfaces. The class talks to ONE local server over TWO unrelated HTTP clients with hand-rolled, non-shared policies. Mapping to Divergent Change: a single conceptual change — "how the SDK reaches the accelerator" (base URL assembly, protocol selection, timeout policy, retry policy, error classification) — forces edits in two stacks that already disagree.

**Maintenance impact:** structural; blast radius = the whole transport layer of the only prod file. Hot path: `/health` runs before *every* prove (`createChonkProof:387` → `checkAcceleratorStatus`), `/prove` is the core operation.

**Concrete evidence:**
- `/health` uses native `fetch` with `AbortSignal.timeout(2000)`, a manual `Promise.any` dual-probe, and a manual `setTimeout(…,1000)` single retry: `accelerator-prover.ts:259-288`.
- `/prove` uses `ky.post` with `timeout: ms("10 min")`, `retry: 0`, and `HTTPError`-based error handling: `accelerator-prover.ts:414-439`.
- Divergent error model: `/health` failure is a bare `catch {}` collapsing to a status enum (L284, L370); `/prove` failure is `instanceof HTTPError` with `err.response.status` / `err.data` inspection (L425-430). Two different notions of "request failed" for the same server.
- Base-URL logic is *also* split: `/health` rebuilds URLs inline from host+port (L254-255) while `/prove` reads the `#acceleratorBaseUrl` getter (L414, getter at L224-229) — two formulations of the same endpoint string.

**Why it harms future change:** Add mTLS, a custom header, a proxy, an auth token, or change the retry/backoff policy and you must implement it twice, in two libraries, keeping their semantics aligned by hand. The URL split (L224-229 vs L254-255) means a host/port format change (e.g. IPv6 bracketing, a path prefix) must be mirrored or `/health` and `/prove` will hit different addresses — a silent foot-gun the type system won't catch.

**Smallest safe refactoring:** Extract Function — one private `#acceleratorBaseUrl(protocol)` already half-exists; route both `/health` and `/prove` through a single `ky` instance (or a single thin transport helper) configured per-endpoint with timeout/retry, so the URL, protocol, and error-classification live in one place. Consolidating onto `ky` (already a dependency) removes the `fetch` branch entirely.

**What disappears:** The second HTTP client, the duplicated URL assembly, and the two-flavored error model — one transport, one place to change policy.

**Instances:** `accelerator-prover.ts:224-229` (URL getter), `:254-255` (inline URL dup), `:259-288` (fetch+manual retry), `:414-439` (ky+HTTPError).

---

## Finding 3 — `#probeAndParseHealth` is a Long Method braiding three responsibilities

**Smell:** Long Method (Bloater). ~123 LOC (`accelerator-prover.ts:252-374`) doing (a) dual-protocol transport + retry, (b) JSON parsing with its own error arm, and (c) multi-version-vs-legacy protocol *business* logic — three reasons to change in one method, with the cache-write helper (`cacheAndReturn`) threaded through six distinct return sites.

**Maintenance impact:** structural; local to the prod file but central to it. Hot path (runs before every prove). Six exit points (L294, L315, L337, L354, L363, L372) each constructing a different union arm by hand — high cognitive load, easy to add a 7th arm inconsistently.

**Concrete evidence:**
- Transport + retry: `:259-289`.
- Protocol caching side-effect interleaved with parsing: `:302` sets `#acceleratorProtocol`, `:314` resets it on bad JSON, `:371` resets on outer catch — the field is mutated from three points inside one method.
- Business logic (multi-version vs legacy version-match): `:327-369`.
- Six `cacheAndReturn({...})` call sites, each assembling a union arm inline: `:294`, `:315`, `:337`, `:354`, `:363`, `:372`.

**Why it harms future change:** Adding a third wire protocol, a new `reason`, or changing the version-comparison rule means navigating transport + parse + cache concerns simultaneously; the `#acceleratorProtocol` mutations scattered across the method make it hard to reason about which exit leaves the protocol pinned vs reset (the comments at L292-293, L312-313, L370 exist precisely to paper over this — Comments-as-deodorant).

**Smallest safe refactoring:** Extract Method into three: `#probeHealthEndpoint()` → `{res, protocol}` (transport+retry), `#parseHealthBody(res)` → typed data or error, and `#classifyHealth(data, protocol, sdkVersion)` → `AcceleratorStatus` (pure, the version logic). Caller composes them and owns the single cache-write + the single `#acceleratorProtocol` assignment.

**What disappears:** The three-way mutation of `#acceleratorProtocol` from inside one method, the six inline union constructions, and the deodorant comments — version logic becomes pure and unit-testable without a fetch mock.

**Instances:** `accelerator-prover.ts:252-374`.

---

## Finding 4 — Bare `catch {}` flattens *every* health failure to `offline` (error-as-control-flow)

**Smell:** error-as-control-flow (named analog; couples to the false `available:false, reason:"offline"` contract). Mapping: distinct failure modes — DNS error, connection refused, TLS handshake failure, abort/timeout, an unexpected throw inside the parsing branch — all funnel through one argument-less `catch` (`accelerator-prover.ts:370-373`) and are reported to the caller as the single most-specific-sounding reason, `"offline"`, which the type explicitly documents as "**both** the HTTP and HTTPS probes failed — the accelerator isn't running" (`:73-74`).

**Maintenance impact:** structural (it shapes the public `reason` contract consumers switch on); local in code. The `reason` enum is part of the weighted consumer surface (`MIGRATION.md:67-76` shows consumers switching on it).

**Concrete evidence:**
- Outer `catch` with no binding, unconditionally returning `offline`: `accelerator-prover.ts:370-373`.
- The same flattening at the inner probe-retry `catch` (`:284`) — a thrown non-network error during `.json()` chaining or an `AbortError` is indistinguishable from connection-refused.
- The `offline` arm's own doc-comment over-claims the cause (`:73-74`), so the lie is baked into the type.

**Why it harms future change:** When a consumer (or maintainer) wants to surface "accelerator is up but its TLS cert is broken" vs "accelerator isn't running", the information has already been discarded at the catch — you cannot add that distinction without re-plumbing the probe to capture the rejection reason. A bug where `/health` 500s intermittently will be reported to users as "offline", sending them to check whether the app is running rather than its logs. The catch also masks genuine programmer errors (a thrown `TypeError` from a refactor) as a benign offline state — they vanish instead of failing a test.

**Smallest safe refactoring:** Bind the error (`catch (err)`) and either (a) inspect the `AggregateError`/rejection to distinguish abort/timeout from connection-refused before choosing the `reason`, or (b) at minimum `logger.debug(err)` so the discarded cause is observable. Narrow the `offline` doc-comment to match what the code can actually prove.

**What disappears:** The silent collapse of unrelated failure modes into one reason; the over-claiming comment; masked programmer errors.

**Instances:** `accelerator-prover.ts:284`, `:370-373`; contract doc `:73-74`.

---

## Finding 5 — `AcceleratorProtocol` is module-exported and doc-promised "exported" but missing from the barrel

**Smell:** Doc-contract drift + export inconsistency (analog of Divergent Change across the package boundary). Mapping: `MIGRATION.md:83` tells consumers "The new `AcceleratorProtocol` type (`"http" | "https"`) is exported for convenience" — but the package barrel (`index.ts`, the package's `main`/`types` entry) re-exports only 5 of the 6 module-level type/value exports, omitting exactly that one. So the documented import fails for any consumer importing from the package root.

**Maintenance impact:** local (one missing line) but on the weighted public boundary — a consumer following MIGRATION verbatim hits a "no exported member 'AcceleratorProtocol'" compile error. Low change frequency, high "first 5 minutes" annoyance.

**Concrete evidence:**
- Module exports it: `accelerator-prover.ts:46` (`export type AcceleratorProtocol = "http" | "https";`).
- Barrel re-exports `AcceleratorConfig, AcceleratorPhase, AcceleratorPhaseData, AcceleratorProverOptions, AcceleratorStatus` and the class — **no `AcceleratorProtocol`**: `index.ts:1-8`.
- Doc explicitly promises it is exported: `MIGRATION.md:83`. (`AcceleratorStatus`'s arms reference `AcceleratorProtocol` by name at `:69,82,90`, so a consumer destructuring the union's `protocol` field has a legitimate reason to want the named type.)

**Why it harms future change:** The barrel is the package's public contract; an item that's `export`ed from a module but not surfaced in the barrel is in a limbo state — present in `.d.ts` via the union's structural use but not importable by name. Either it's public (add it) or it isn't (fix the doc). Leaving it half-exported means future maintainers can't tell whether removing/renaming it is a breaking change.

**Smallest safe refactoring:** Add `AcceleratorProtocol` to the `export type { … }` list in `index.ts:1-7` (matches the doc). One line.

**What disappears:** The doc/barrel contradiction and the un-importable named type.

**Instances:** `index.ts:1-8` (omission), `accelerator-prover.ts:46` (source), `MIGRATION.md:83` (false promise).

---

## Finding 6 — `setForceLocal` is a shipped, consumer-used public method absent from the README API table

**Smell:** Doc-contract drift (under-documentation of the public surface). Mapping: the README's "API Reference" method table (`README.md:61-66`) enumerates 4 methods as *the* public method set, but the class exposes a 5th mutator, `setForceLocal(force)`, which is genuinely public and *already consumed by a sibling package* — so the table misrepresents the surface as smaller than it is.

**Maintenance impact:** local; weighted boundary. The omission means the only doc that covers `setForceLocal` is the bundled SKILL.md (`:109-114`), not the README that npm renders. Confirmed live consumer dependency, so it's not dead/speculative.

**Concrete evidence:**
- Public method, no doc in README table: `accelerator-prover.ts:219-222` (`setForceLocal(force: boolean)`).
- README method table lists only `checkAcceleratorStatus`, `setAcceleratorConfig`, `setOnPhase`, `createChonkProof`: `README.md:61-66`.
- Real consumer: `packages/playground/src/aztec.ts:251` (`state.prover?.setForceLocal(mode === "local");`).
- It *is* documented in the bundled skill (`SKILL.md:109-114`), so the README is the inconsistent surface, not the method.

**Why it harms future change:** A maintainer pruning "unused" API might delete or change `setForceLocal`'s signature trusting the README's 4-method list, silently breaking the playground's local-vs-accelerated toggle (the package's own demo). Inversely, a consumer never learns the supported force-WASM path exists and reaches for a hack. The README table is the contract that gates "is this a breaking change?" — an incomplete table corrupts that judgment.

**Smallest safe refactoring:** Add one row to the README method table (`README.md:61-66`): `setForceLocal(force)` → `void` → "Force WASM proving, bypassing accelerator detection." Matches the existing JSDoc at `:219`.

**What disappears:** The README-vs-(SKILL+code+consumer) mismatch; the false impression that the public method set is 4 methods.

**Instances:** `README.md:61-66` (omission), `accelerator-prover.ts:219-222` (source), `SKILL.md:109-114` (where it IS documented), `packages/playground/src/aztec.ts:251` (consumer).

---

### Non-findings (considered, rejected with reason)

- **Temporal Coupling `#statusCache` ↔ `#acceleratorProtocol`** (suggested in brief): both ARE mutated together, but `setAcceleratorConfig` resets both as a pair (`:210-211`) and the probe owns both — the coupling is contained in one class and one method-set with an explicit pairing comment. No cross-file change-amplification and no order-dependent public API. Below the "costs something measurable" bar; folded into Finding 3's note about scattered `#acceleratorProtocol` mutation rather than raised separately.
- **Primitive Obsession on ports/host (`number`/`string`)** — three loose primitives in `AcceleratorConfig`, but they're a stable, documented config triple consumed once at construction; no arithmetic, no validation logic duplicated across sites, no clump passed around as a unit beyond the single config object. Wrapping in a value object is speculative generality here. Not flagged.
- **Long Parameter List on `createChonkProof`/internal provers** — internal `#proveLocally`/`#fallbackToWasm` take `(steps, logLabel)`; 2 params, not a clump. Public `createChonkProof` takes one arg. No finding.
- **`logger.ts` Lazy Class** — 2 lines re-exporting a configured LogTape logger; it's an intentional single-import seam used across the file. Not a smell.
- **`createChonkProof` Long Method** (suggested in brief): ~77 LOC but it's a linear pipeline with clear phase-emit landmarks and the two prove-paths already extracted (`#proveLocally`, `#fallbackToWasm`). The 403 branch is the only nested complexity and is self-contained. Borderline; does not clear the bar given Finding 3 already covers the worse method. Noted, not certified.
