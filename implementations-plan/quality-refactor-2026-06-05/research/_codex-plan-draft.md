**Ordering**

I would sequence this as: characterization harness first, then the cheap converged wins, then the two value objects, then the remaining non-architectural structural refactors, then the medium-risk coordinators, and only then the two deepest core-path splits. That order minimizes simultaneous semantic movement on the hottest paths: `/prove`, origin authorization, version resolution, tray status, HTTPS/cert startup, and the SDK’s public proving/status contract.

The hardest behavior-preservation work is Q2, then Q1, then the “looks-safe-but-isn’t” items Q9, Q10, Q14, and Q7. Q9 is not behavior-preserving if you silently normalize the swallowed save error. Q14 can change which origins auto-approve. Q10 can change tray timing even if displayed text stays the same. Q7 can silently change Safari recovery policy because startup and settings currently diverge.

**Dependency Graph**

- Phase-0 characterization is a prerequisite for Q2, Q3/Q11, Q10, Q12/Q5, and Q14.
- Q15 should land before any auth extraction; it is one UX/security contract shared by `authorize_origin` and the auth popup timeout.
- Q8 should precede Q2 and Q12; it stabilizes the HTTP error/header protocol before server and SDK refactors reopen it.
- Q9 should precede Q6 and Q7; both workflows mutate config and should build on one helper, not six copy-pastes.
- Q10 should precede Q2; it touches both the server emit sites and the tray consumer, and otherwise conflicts badly with the server split.
- Q3 should precede Q11 and absorb the `versions_to_evict` and `bb_asset_name` minors.
- Q12 should precede Q5; otherwise you risk extracting the SDK around a contract you already know you are deleting.
- Q4 is best before Q6 so `UpdateCoordinator` depends on a stable crash-recovery facade, not platform-specific free functions.
- Q1 must precede Q2; otherwise `ProveWorkflow` gets extracted against the wrong seam.
- Residual minor cleanups should mostly be folded into owning PRs; a single early sweep would maximize merge conflicts for little leverage.

**Standard validation for every PR**

`cd packages/accelerator/src-tauri && cargo test --lib`; `bun run test`; `bun run lint`; `bun run lint:actions`. For SDK-contract or desktop-bootstrap PRs, also run `bun run --cwd packages/sdk test:e2e` and the accelerator UI/WebDriver jobs in CI.

**Phases**

1. **Phase 0: characterization harness.** Ship this as two PRs: `rust-hot-path-characterization` and `sdk-contract-characterization`. Add Rust tests that pin exact `/prove` JSON error bodies, auth timeout/deny/auto-approve behavior, status-callback ordering, and success-path response headers using a fake `bb` executable via `BB_BINARY_PATH` rather than a live binary. For `resolve_version`, add the smallest possible seam for a fake downloader/cache adapter if needed; this is the one place a seam-only change is justified before refactoring. On the SDK side, promote the existing mocked-`fetch` tests into golden contract tests for offline, legacy mismatch, multi-version download-needed, HTTPS fallback, denied fallback, and native success phase traces. Fowler move: seam-only `Extract Method` / `Introduce Parameter Object` where tests need it. Rollback: never remove these tests; later PRs revert against them.

2. **Phase 1: cheap converged wins: Q15, Q8, Q9, Q10.** Q15 is its own micro-PR: `Introduce Constant` for the shared auth timeout used by both the server and popup. Q8 is another micro-PR: `Replace Primitive with Object` for the error DTO plus header constants, but keep serialized field names and header literals identical. Q9 must be handled carefully: first add a regression test proving that `respond_update_prompt` currently swallows `config::save` failure while the other config mutators propagate it, then refactor to explicit helpers such as `mutate_config` and `mutate_config_best_effort`; do not “fix” the divergence inside the refactor PR. Q10 is a separate PR: `Replace Primitive with Object` for `ServerStatus`, with tests pinning exact displayed strings and busy/idle transitions so tray timing does not change. Rollback for all four is trivial because each PR is self-contained and behavior remains externally identical.

3. **Phase 2: value objects and the SDK major bump: Q3 and Q12.** Q3 should be one PR centered on `AztecVersion`: `Replace Primitive with Object`. Its constructor must preserve the current accepted-string behavior, not tighten it to prettier semver; the current tests already allow values like `4.2.0-aztecnr-rc.2` and `1.2.3-alpha_beta`. Fold the `versions_to_evict` re-parse and duplicated `bb_asset_name` formatting into this PR. Q12 is the deliberate breaking-change PR: replace flat `AcceleratorStatus` and `(phase, data?)` with discriminated unions, update `packages/sdk/src/index.ts`, migrate internal playground consumers in the same PR, and publish a migration document plus release notes. Rollback: before publish, revert the PR cleanly; after publish, do not unpublish the major, patch forward with compatibility guidance if needed.

4. **Phase 3: non-architectural structural extractions: Q11, Q13, Q14, plus adjacent minors.** Q11 becomes a versions-only PR using `Extract Method`: split `download_bb` into download, digest, install, and macOS post-process steps, keeping cleanup semantics exact. Q13 is a build-script PR using `Replace Conditional with Table`; pin the current matrix and archive-layout canary first. Q14 is a security-sensitive micro-PR using `Substitute Algorithm`: canonicalize once and derive auto-approval from the parsed host, but only after adding negative tests for path/query/userinfo, IPv6, extension origins, and localhost aliases. Fold in adjacent minors here only when they belong to the same seam; do not do a repo-wide cleanup sweep yet.

5. **Phase 4: medium-risk coordinators: Q7, Q4, Q6, Q5.** Q7 should be an `Extract Class` PR for `SafariSupportManager`, but preserve the current policy split by exposing explicit entry points such as `startup_preflight` and `enable_from_settings`; otherwise the refactor will silently change recovery behavior. Q4 should introduce a crash-recovery interface/facade so callers no longer know platform details; pin the Windows “disable returns bool and updater must abort if false” behavior before touching it. Q6 then becomes an `Introduce State Object` PR for the update flow, built on Q9 and preferably Q4; pin pending-update storage, prompt display rules, “later” behavior, and auto-install behavior first. Q5 is last in this tranche: `Extract Method` plus `Extract Class` for SDK health probing/parsing and phase reporting, after Q12 has already frozen the new public types.

6. **Phase 5: highest-risk core-path architecture: Q1 then Q2.** Q1 is its own PR: `Extract Class` for `HeadlessState` and `GuiCallbacks`, plus explicit constructors for headless and desktop state. Add characterization tests that pin headless auto-approve/deny semantics, health behavior, and desktop callback wiring before changing the shape. Q2 is the final and riskiest PR: `Extract Class`, `Move Method`, and `Extract Method` to carve `ProveWorkflow` and split `server.rs` into handler/workflow/bind/TLS modules. The tests added in Phase 0 are the guardrails here: preserve auth-before-body-read, semaphore ordering, version-download/proving status transitions, response headers, and idle-reset semantics. Rollback should be “revert the whole PR,” not hand edits.

If any minor-bucket items remain after those phases, do one final cosmetic-only sweep PR. I would absorb most of them instead: version minors into Q3, SDK fallback duplication into Q5, AppState clone-stutter into Q1, window-helper duplication into Q7, and home-dir inconsistencies into Q4.

**SDK major bump**

Make Q12 the only intentionally breaking PR in the stack. The PR should include: the unionized types, updated exports, internal playground migration, README updates, a dedicated migration doc with before/after examples, and release notes that call out `status.available` checks and `onPhase(phase, data)` consumers. Because the checked-in SDK `package.json` still carries a placeholder version, treat the actual semver bump as release-pipeline work and document the target major explicitly in the PR description.

**Characterization harness**

Do not depend on a live `bb` or live GitHub/Aztec endpoints. For Rust, use in-process Axum router tests, `tokio::time::pause` for timeout paths, a fake `bb` executable for success-path proving, and a tiny fake downloader seam only where `resolve_version` otherwise hard-calls the network. For the SDK, keep using mocked `fetch`/`ky` with fixed response fixtures; the protocol is small enough that recorded traffic adds little value over deterministic golden objects.

**Security & Adversarial Considerations**

- Preserve auth-before-body-buffering and before any `bb`/download side effects; moving auth later weakens DoS posture and changes failure precedence.
- Preserve the current version-validation guard before any path derivation or network/file activity.
- Preserve the “generic outward error, detailed server log” rule for `bb` failures; Q8 must not leak paths, stderr, or witness data.
- Treat Q14 as security-sensitive; parser unification must not broaden auto-approval beyond today’s localhost set.
- Treat Q7 and Q4 as security-sensitive because they touch cert trust, HTTPS startup, and update/crash-recovery sequencing.
- Preserve the cert invariants: CA key never on disk, staged trust-then-swap rotation, and legacy CA-key deletion.
- Preserve updater signature verification and the Windows disarm/rearm ordering around install.
- In Q12/Q5, ensure the new SDK unions cannot be misread by consumers as “available by default”; make the denied/unavailable states explicit.

**Assumptions**

Facts:
- `AppState` is currently one mixed runtime struct with optional GUI callbacks, config/auth, and semaphore fields, and both desktop and headless entry points construct only subsets of it: [server.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:32), [main.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:347), [server main](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/src/main.rs:62).
- `/prove` currently authorizes first, then reads the body, then acquires the semaphore, resolves version, calls `bb::prove`, and sets `x-prove-duration-ms` on the response: [server.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:487).
- Tray animation is currently driven by substring matching on status text, while the server emits human-readable status strings: [main.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:347), [server.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:434).
- Version semantics are already split across tier parsing, sort-key parsing, validation, and download naming: [versions.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:15), [versions.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:129), [versions.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/versions.rs:257).
- The SDK still exports flat `AcceleratorStatus`, string `AcceleratorPhase`, and `onPhase(phase, data?)`, and the playground imports those types directly: [accelerator-prover.ts](/Users/alejoamiras/Projects/aztec-accelerator/packages/sdk/src/lib/accelerator-prover.ts:10), [index.ts](/Users/alejoamiras/Projects/aztec-accelerator/packages/sdk/src/index.ts:1), [ascii-animation.ts](/Users/alejoamiras/Projects/aztec-accelerator/packages/playground/src/ascii-animation.ts:1), [aztec.ts](/Users/alejoamiras/Projects/aztec-accelerator/packages/playground/src/aztec.ts:1).
- Cert handling is deliberately keyless-on-disk and fail-closed during rotation: [certs.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:43), [certs.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/certs.rs:254).
- Release discipline already expects `bun run test` and `cargo test --lib`: [RELEASE_RUNBOOK.md](/Users/alejoamiras/Projects/aztec-accelerator/docs/RELEASE_RUNBOOK.md:5).

Inferences:
- The checked-in desktop crate still says `1.0.4-rc.1`, so I assume these PRs land on the post-1.0.4 development line rather than on the already-shipped stable cut: [Cargo.toml](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/Cargo.toml:1).
- `resolve_version()` and `prove()` will need tiny test seams or fake executables to get full characterization coverage without live network or live `bb`.

Asks:
- Confirm the intended next published SDK major number; the checked-in SDK package version is placeholder-style and does not itself tell me the release target.
- Confirm whether you want the residual minor bucket absorbed into adjacent PRs, which I recommend, or forced into one separate cleanup PR.

Planning pass only; I read the audit and source files but did not run the test suite.