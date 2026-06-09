## Cluster verdict — C2 core-bb-versions

3 findings, all **structural** (no architectural rot). The crate has clearly been through a quality refactor already (Q2/Q3/Q11): `AztecVersion` is a real value object, `versions_to_evict` reads precomputed fields, and the download flow is split into `download_tarball` / `verify_digest` / `install_version_dir`. What remains: (1) `download_bb`'s post-extract tail mixes a cross-platform install step with a macOS-only Gatekeeper-repair step in one function (Divergent Change + Long Method); (2) the two macOS subprocess blocks (`xattr`, `codesign`) duplicate the spawn→match→log skeleton (Duplicate Code); (3) the platform-string mapping is split across three `cfg`-gated functions that must change in lockstep (Shotgun Surgery analog). Severity spread: 1 moderate, 2 low. No false-positive temptations taken on the heavily-commented refactor markers (consistent convention, no measurable change cost) or the digest/hex helpers (single-use, not duplicated — the `hex::encode` in `commands.rs:132` is a window-label hash, an unrelated concern).

---

## Finding 1 — `download_bb` mixes cross-platform install with a macOS Gatekeeper-repair tail

1. **Title** — macOS xattr-clear + ad-hoc codesign bolted onto the cross-platform `download_bb` orchestrator.

2. **Smell** — **Divergent Change** (Change preventer) + **Long Method** (Bloater). `download_bb` (versions.rs:342–420, ~78 LOC) has two unrelated reasons to change living in one function: the *download/verify/install/chmod* pipeline (cross-platform, lines 348–376) and the *macOS code-signing repair* (lines 383–416, a 33-line `#[cfg(target_os = "macos")]` block — 42% of the body). Divergent Change because a future edit to the signing strategy (e.g. switch to a real Developer-ID cert, add notarization, or add a Windows Authenticode equivalent) forces edits inside the same function that owns the platform-neutral cache install, and vice-versa.

3. **Maintenance impact** — **structural**; blast radius 1 file / 1 function, but it is the **hot path** for every first-download of a new bb version (every version bump, nightly, RC). Change frequency: moderate-high — the comment at 379–382 documents that this block exists to work around chmod-invalidating-the-signature, exactly the kind of platform quirk that gets revisited. Today there is no symmetric Windows branch; the moment one is added, this function balloons further.

4. **Concrete evidence** —
   - Orchestrator + chmod: versions.rs:348–376.
   - macOS-only tail begins versions.rs:383 (`#[cfg(target_os = "macos")]`) and runs to 416.
   - xattr subprocess: versions.rs:385–395.
   - codesign subprocess + cleanup-on-failure (`remove_dir_all(&version_dir)` at 405 and 411): versions.rs:397–415.
   - The function already delegates the other three phases (`download_tarball` 358, `verify_digest` 364, `install_version_dir` 369) — the macOS tail is the lone inline phase that was not extracted, making it the odd-one-out.

5. **Why it harms future change** — Adding Windows Authenticode signing or macOS notarization means editing the body of the same function that does the cross-platform `version_dir`/chmod work, raising merge-conflict and accidental-breakage risk on the critical download path. A reader auditing the cache-install logic must mentally skip 33 lines of unrelated Gatekeeper code. The cleanup-on-failure (`remove_dir_all`) is now coupled to the signing step specifically, so the "don't cache a broken binary" invariant is expressed only for the macOS path.

6. **Smallest safe refactoring** — **Extract Method**: pull lines 383–416 into a `#[cfg(target_os = "macos")] fn finalize_macos_binary(version: &str, final_path: &Path, version_dir: &Path) -> Result<(), ...>` (with a no-op `#[cfg(not(target_os = "macos"))]` sibling), called once after chmod. `download_bb` then reads as five symmetric phases: download → verify → install → chmod → platform-finalize.

7. **What disappears** — The 33-line platform wart inside the orchestrator; `download_bb` drops to ~45 cross-platform LOC. The signing concern gets a named home where a Windows/notarization sibling can be added without touching the cache-install path. Divergent Change is resolved: signing changes and install changes no longer share a function.

8. **Instances** — versions.rs:342–420 (function), 383–416 (the tail to extract); the same extraction creates the seam for any future `#[cfg(target_os = "windows")]` signing branch.

---

## Finding 2 — `xattr` and `codesign` blocks duplicate the spawn→match→log subprocess skeleton

1. **Title** — Two near-identical "run a macOS Command, branch on Err / non-zero exit, emit a tracing log" blocks.

2. **Smell** — **Duplicate Code** (Dispensable). The xattr block (versions.rs:385–395) and the codesign block (versions.rs:397–415) share the same shape: build `std::process::Command::new(<tool>).args(...).arg(&final_path).output()`, then a three-arm decision over `Err(e)` / `Ok(out) if !out.status.success()` / ok. The structural skeleton (spawn external tool against `final_path`, inspect `output()`, log on failure) is repeated; only the tool name, args, log severity, and the codesign-only cleanup differ.

3. **Maintenance impact** — **local**; blast radius 1 file / 1 function. Low change frequency, but the duplication is a per-tool tax: any change to how subprocess failures are surfaced (e.g. capture stdout too, switch `tracing::warn` policy, add a retry) must be made twice and the two copies can silently drift (they already differ in severity — xattr logs `warn`, codesign logs `error` — which is correct, but the divergence is exactly what makes copy-paste edits error-prone).

4. **Concrete evidence** —
   - xattr: versions.rs:385–395 — `Command::new("xattr").args(["-cr"]).arg(&final_path).output()`, then `if let Err(e) … else if let Ok(out) … if !out.status.success()`.
   - codesign: versions.rs:397–415 — `Command::new("codesign").args(["--force","--sign","-"]).arg(&final_path).output()`, then `match … Err(e) => … Ok(out) if !out.status.success() => … Ok(_) => {}`.
   - Shared duplicated logic: the spawn-against-`final_path` + status-inspection + failure-log triad.

5. **Why it harms future change** — If a third Gatekeeper-repair command is ever needed (e.g. `spctl`), it gets copy-pasted a third time. If the failure-handling policy changes (capture stderr for xattr too, or make xattr failure fatal like codesign), an editor must remember both sites. Two structurally-parallel blocks invite "fix one, forget the other" bugs.

6. **Smallest safe refactoring** — **Extract Method**: a small helper `fn run_repair_tool(tool: &str, args: &[&str], path: &Path) -> std::io::Result<std::process::Output>` plus a caller-side decision on fatality, or more directly a `fn run_codesign_step(...) -> Result<(), Box<dyn Error...>>` that owns the spawn+match. Best done together with Finding 1's `finalize_macos_binary` extraction so the helper lives beside its only callers.

7. **What disappears** — One of the two copies of the spawn→inspect→log skeleton; future repair tools reuse one code path; the warn-vs-error and cleanup-vs-no-cleanup differences become explicit arguments rather than hand-maintained parallel blocks.

8. **Instances** — versions.rs:385–395 and versions.rs:397–415 (same root cause: ad-hoc inline subprocess handling repeated per macOS tool).

---

## Finding 3 — Platform identity is smeared across three `cfg`-gated functions that must change together

1. **Title** — `bb_binary_name`, `current_platform`, and the macOS-only signing branch encode the same platform matrix in three places.

2. **Smell** — **Shotgun Surgery** (Change preventer, named analog). Adding or changing a supported platform requires edits scattered across multiple independent functions rather than one. This is Shotgun Surgery because "support a new target" touches: (a) `bb_binary_name` versions.rs:140–146, (b) `current_platform` versions.rs:160–181, and (c) the `#[cfg(target_os = ...)]` signing tail at versions.rs:383. There is no single "platform descriptor" — the per-OS knowledge is replicated as separate `cfg` ladders.

3. **Maintenance impact** — **structural**; blast radius 1 file but ≥3 functions that must stay synchronized; also conceptually couples to the sidecar lookup in **bb.rs** (`find_bb` uses `versions::bb_binary_name()` at bb.rs:40, 48 and the Windows-skips-PATH special-case at bb.rs:58–61). Change frequency: low-but-spiky — fires exactly when a new arch/OS is added (e.g. when Aztec ships a new release asset naming, which the test at versions.rs:832 explicitly guards against drifting). `current_platform` (160–181) is also notable: it has **no fallback arm** — an unlisted target fails to compile, which is a correctness/compile concern (noted in passing only).

4. **Concrete evidence** —
   - `bb_binary_name` — Windows vs non-Windows: versions.rs:140–146.
   - `current_platform` — five `#[cfg(all(target_arch, target_os))]` arms: versions.rs:160–181.
   - macOS-only signing gate: versions.rs:383.
   - Downstream platform knowledge in bb.rs: `bb_binary_name()` consumers at bb.rs:40 and 48; the Windows-PATH-hijack carve-out at bb.rs:54–61.
   - The platform string also feeds `download_url` (versions.rs:186–192) and `verify_digest`'s `asset_name` (versions.rs:467) — so a platform-string change ripples to the URL and the digest-asset name too.

5. **Why it harms future change** — Onboarding a new target (say `aarch64-windows`) means: add an arm to `current_platform`, confirm `bb_binary_name` returns `bb.exe`, decide whether the macOS signing tail or a new Windows signing tail applies, and re-check `find_bb`'s Windows-PATH special case — four edits in two files with no compiler enforcement that they agree. Easy to add the URL mapping but forget the signing/lookup side.

6. **Smallest safe refactoring** — **Extract Class / Replace Conditional with a lookup**: introduce a single `Platform` descriptor (or a `const`/`match` resolved once) that exposes `asset_slug()` (`arm64-darwin`), `binary_name()` (`bb`/`bb.exe`), and `needs_codesign()` / `skip_path_lookup()`. The three `cfg` ladders collapse to one source of truth; `download_url`, `verify_digest`, the signing tail, and `find_bb` all read from it.

7. **What disappears** — The replicated per-OS `cfg` ladders; "add a platform" becomes a single descriptor edit instead of a 4-site scavenger hunt; the implicit invariant that all five sinks agree on platform identity becomes structural.

8. **Instances** — versions.rs:140–146, versions.rs:160–181, versions.rs:383, versions.rs:467 (asset name), versions.rs:186–192 (URL); bb.rs:40, bb.rs:48, bb.rs:54–61 (downstream platform-conditioned lookup).

---

### Non-findings (evaluated, deliberately not flagged)

- **Heavy Q2/Q3/Q11 refactor-marker comments** (versions.rs:66–73, 219–221, 344–347, 356–357, 423–425, 462–465, 487–496; prove.rs:42–44). These read like "comments-as-deodorant" / change-log narration, but they are a *consistent house convention* across the crate and impose no measurable change-amplification — per the negative list (conventions aren't smells unless they cost something), NON-FINDING. (If anything they over-document, a cosmetic issue out of scope.)
- **`sha256_hex` / `version_sort_key` / `is_valid_version` helpers** — each single-purpose, single-or-few callers, no duplication. The `hex::encode` at commands.rs:132 hashes an *origin for a window label*, an unrelated concern — NOT duplicate of the digest path. NON-FINDING.
- **Temporal Coupling (download→verify→install→evict)** — the ordering is real but it is *encapsulated inside* `download_bb` and `resolve_version` (prove.rs:64–97), not exposed as a sequence callers must hand-order; the verify-before-install invariant is enforced in one place. No client can get the order wrong. NON-FINDING.
- **`AztecVersion` Primitive Obsession** — already remediated (Q3 value object, versions.rs:74–129). The few remaining `&str`/`version_bb_path(version: &str)` boundaries (versions.rs:149) are the intentional `Deref`-to-`&str` sinks. NON-FINDING.
- **Long Parameter List / Data Clumps (path+version+url)** — checked: functions pass at most `(version)` or `(version_dir, bytes)`; no recurring 3+ clump travels together. NON-FINDING.
