# c3-version-supply-chain — Claude (Opus) raw findings

**Pipeline strengths verified (on record, so negatives aren't re-derived):** the traversal guard holds —
`is_valid_version` (`version_policy.rs:266-275`) + the `AztecVersion` newtype make every path/URL sink
structurally unreachable by an unvalidated string; the charset (`[A-Za-z0-9._-]`, no `..`, no
leading/trailing dot, ≤128) also excludes Windows reserved device names, URL metacharacters, non-ASCII.
Digest verification is ordered before any filesystem write (`downloader.rs:37 → :43 → :55`) and fails
closed on a missing digest. The tar extractor pins the destination filename, rejects non-`Regular`
entries, and `CappedReader` counts DECOMPRESSED bytes across skipped entries. The marker is checked
before EVERY execution (`bb.rs:37-40`), not only at install. No bypass of any of these was found.
All three findings are about what happens AFTER those guards pass.

## F-C3-1 — An approved origin can permanently exhaust host disk via the version cache (unbounded retention + eviction that only runs post-download and skips anything <5 min old)

**Impact.** Availability, host-wide (a full disk breaks the whole machine; the accelerator's own
config/cert writes then fail). Persistent — there is NO "clear cache" command in the tray or Tauri
surface (grepped `commands.rs`/`tray.rs`), so recovery is manual `rm -rf ~/.aztec-accelerator/versions`.
Vector: network, any once-approved origin (or the accepted absent-Origin local path). Complexity low.
Privileges none beyond a one-time approval. No per-request user interaction.
**Confidence** high. **CWE-770** / CWE-400, OWASP A04:2021.

**Trace.** `prove.rs:249-253` untrusted `x-aztec-version` → `:233` `authorize_origin` (only gate) →
`:238` `try_enter` takes 1 of `MAX_INFLIGHT_PROVE = 8` (`server.rs:37`) — a CONCURRENCY cap, not a rate
cap → `:67` → `version_policy.rs:77-86` charset gate → `:82` → `version_policy.rs:240-260`
`check_version_selectable` (canonical semver, no build metadata, not on the EMPTY
`KNOWN_VULNERABLE_VERSIONS` `:194`) → `:108` → `cache_layout.rs:209` uncached ⇒ download →
`:275` → `downloader.rs:20`, `:117-151` streams ≤64 MB, `:55` → `install_version_dir` (`:270-311`)
publishes a permanent `~/.aztec-accelerator/versions/<ver>/` holding a ~37 MB `bb` (measured on host).
**The download completes BEFORE the body is interpreted** — `bb::prove` is only called at `prove.rs:329`,
so a 2-byte junk body still pays for the full download; no valid witness is ever needed.

**Why nothing reclaims it.** `cleanup_old_versions` has exactly ONE call site: the detached
`tokio::spawn` at `prove.rs:287-299`, which runs only after a SUCCESSFUL download — stop attacking and
nothing ever reclaims. `version_policy.rs:299` skips any dir whose mtime is within
`CACHE_ENTRY_ACTIVE_WINDOW = 5 min` (`downloader.rs:215,219-230`), so during a burst every version
downloaded in the last 5 minutes is exempt from every cleanup that burst triggers. `version_policy.rs:158`
skips eviction entirely for `NetworkTier::Mainnet`, whose limit is `None` (`:52`) — and
`NetworkTier::from_version` (`:30-44`) classifies ANY prerelease label it does not recognize as Mainnet
(only `nightly*`, `devnet*`, `rc*` are bounded), so `-alpha-testnet.*`, `-staging.*`, `-beta.*` and every
plain `X.Y.Z` land in the never-evicted bucket.

**Missing control.** No total-size/count ceiling on the versions dir; no rate limit on NEW-version
downloads; a retention policy that FAILS OPEN (unknown tier ⇒ keep forever); eviction event-driven off a
single success path rather than periodic.

**Exploit.** Approved origin enumerates aztec-packages tags via the public GitHub API, filters to
non-`nightly`/`devnet`/`rc` labels, fires `POST /prove` with `x-aztec-version: <tag>` and a 2-byte body,
8 at a time. Each 429s/500s at the proving step — irrelevant, the 37 MB binary is already installed and
never evicted. Hundreds of qualifying historical releases × 37 MB ≈ 15–40 GB, permanent.

**Why mitigations fail.** 8-way concurrency ≠ completed-download bound; the 64 MB/512 MB caps bound a
SINGLE archive; tier limits don't apply to the Mainnet catch-all and are defeated inside the 5-minute
window where they do. The in-code "self-healing — a later cleanup evicts them" comment
(`downloader.rs:210-214`) is false for Mainnet and requires another successful download otherwise.
Digest verification is irrelevant — the attacker installs AUTHENTIC binaries.

**Instances.** `version_policy.rs:52`, `:30-44`, `:158`, `:299`; `downloader.rs:215,219-230`;
`prove.rs:287-299`.

## F-C3-2 — `x-aztec-version` lets a remote origin choose WHICH historical native binary is executed over the private witness (no floor, no allowlist, compile-time-only revocation)

Explicitly NOT SEC-02 (which is about trusting GitHub's digest): this holds even with a perfect digest
chain, because the attacker selects an AUTHENTIC binary.

**Impact.** Integrity + Confidentiality — the selected binary is executed as a child with the user's full
privileges (`bb.rs:206,229`) and handed the private witness (`bb.rs:198`, `--ivc_inputs_path`). The SAME
origin controls both the binary choice and the input bytes, so a memory-safety defect in any historical
release is reachable with attacker-chosen input. Blast radius: the accelerator's process context —
loopback listener, config store, local-CA trust integration, autostart, updater state; escalates "a web
page" to "code running as the user". Vector network; complexity HIGH (needs an exploitable historical
`bb`); privileges none beyond one approved origin.
**Confidence** high for the trace and the absence of any bound; **moderate** for realized impact — it is
contingent on some historical barretenberg release having an input-reachable memory-safety defect. The
agent did NOT audit barretenberg and makes no claim that one exists; what is certain is that the design
grants the choice. **CWE-829** / CWE-1104 / CWE-757, OWASP A06:2021 / A08:2021.

**Trace.** `prove.rs:249-253` → `:82` → `version_policy.rs:240-242` → `check_version_against`
(`:247-260`): the ONLY semantic gate is `denylist.contains(&requested)` against the empty
`KNOWN_VULNERABLE_VERSIONS` (`:194`); everything else is well-formedness → `prove.rs:275` →
`downloader.rs:20` → `release_metadata.rs:54-60` builds
`…/releases/download/v{VERSION}/barretenberg-{PLATFORM}.tar.gz` from the attacker's string (any tag in
repo history) → `downloader.rs:55` publishes; **on macOS the app ad-hoc RE-SIGNS it** (`:83-86`), i.e.
manufactures local trust for whatever it fetched → `prove.rs:329` → `bb.rs:191` → `find_bb(Some(v))`
(`:37-40`) → `verify_cached_bb` confirms bytes match the marker, NOT that the version was ever tested →
sink `bb.rs:206` `Command::new`, `:229` `spawn`, with the attacker's body already written.

**Missing control.** No lower bound relative to the bundled version, no allowlist of tested versions, no
bound on deviation from `state.bundled_version`, and no RUNTIME-updatable revocation channel —
`KNOWN_VULNERABLE_VERSIONS` is a compile-time `const`, so shipping a revocation needs a full app release
AND the user accepting the (declinable) auto-update.

**Why mitigations fail.** The digest check proves AUTHENTICITY, not safety — it is precisely what makes
this work. `check_version_selectable`'s canonical-semver requirement rejects `latest`/`5`/`5.0` and build
metadata but imposes NO ordering constraint; the doc at `version_policy.rs:226-231` states the no-floor
decision explicitly ("many Aztec releases share one bb"), which argues against a strict `>= bundled`
floor but not against ANY bound. `verify_cached_bb`/the marker are cache-integrity controls, not version
selection. The 5-minute active-window and in-use exemptions actually HELP the attacker keep the chosen
old binary resident.

**Instances.** `version_policy.rs:194`, `:240-260`; `release_metadata.rs:54-60`; `bb.rs:191-229`.

## F-C3-3 — Every version-bearing `/prove` re-hashes the whole cached binary TWICE, synchronously on the async runtime (~370,000× request-to-work amplifier)

**Impact.** Availability — both listeners (all endpoints incl. `/health` and HTTPS, since the hashes run
on runtime worker threads) plus host CPU shared with the in-flight proof. Degrades, does not destroy;
self-recovers when the flood stops. Vector network; complexity low; one approved origin.
**Confidence** high for the path and the double-hash; moderate for magnitude (SHA-NI, 37 MB binary).
**CWE-400** / CWE-1050, OWASP A04:2021.

**Trace.** `prove.rs:249-253` names an already-cached non-bundled version → `:238` 8 permits, no rate
limit → `:268` `resolve_version` called SYNCHRONOUSLY from the async handler (plain `fn`, not
`spawn_blocking`) → `:108` → `cache_layout.rs:209-219` → `verify_bb_entry` (`:185-204`) → `sha256_file`
(`:99-113`), a blocking read loop over the whole 37 MB binary on a tokio worker — **hash #1** →
`prove.rs:329` → `bb.rs:191-192` → `:38` `verify_cached_bb` → `sha256_file` again over the same unchanged
bytes — **hash #2**. ~74 MB of hashing + 74 MB of reads per ~200-byte request, with hash #1 running
OUTSIDE the single prove permit, so up to 8 run in parallel, blocking up to 8 runtime workers.

**Missing control.** No rate limit (only concurrency); the blocking hash is not moved off the runtime via
`spawn_blocking`; no memoization of "(path, mtime, size, digest) verified moments ago".

**Why mitigations fail.** `MAX_INFLIGHT_PROVE` caps simultaneity not throughput; body caps are irrelevant
(the body stays tiny); the single `prove_semaphore` serializes only `bb::prove` (hash #2), explicitly NOT
`resolve_version` (hash #1) — that decoupling is the intended A1 slowloris fix and is exactly what makes
hash #1 parallelizable here. The 429 path never engages because the requests complete quickly.

**Instances.** `prove.rs:108`; `bb.rs:38` via `prove.rs:329`; `cache_layout.rs:99-113`.

## Considered and REJECTED (recorded so they aren't re-derived)

- **Path traversal / version-string abuse** — no escape. Charset + newtype cover `..`, separators,
  absolute paths, leading/trailing dots, non-ASCII, empty, >128; canonical semver additionally rejects
  `_`-bearing strings and guarantees the dir name starts with a digit (Windows device names unreachable).
  Staging dirs are dot-prefixed, which the version charset forbids — no version/stage collision.
- **Case-insensitive-FS collision** (`5.0.0-RC.1` vs `-rc.1`): `read_bb_marker_at` compares the marker's
  `version` byte-exactly (`cache_layout.rs:169-171`), so the aliased entry fails verification and the
  re-download 404s on a case-sensitive GitHub tag. Clean error, not confusion.
- **Marker forgery by a local same-user process** — real (the marker is unauthenticated JSON, and
  `cache_layout.rs:208` overstates it as "the SINGLE authority the runtime trusts"), but such a process
  can already read the witness out of the 0700 workspace and own the session. No independent
  security-property violation. Doc-accuracy nit only.
- **Concurrent same-version publish race** — both binaries are byte-identical (same digest-verified
  asset); worst case is a transient ENOENT/failed remove, self-recovering. Correctness, not security.
- **B1/B2 eviction TOCTOU** (a truly-old in-use version evicted by a concurrent detached cleanup) —
  documented in-code (`downloader.rs:209-214`, `version_policy.rs:296-299`), availability-only,
  recoverable by re-download. Its aggregate consequence is folded into F-C3-1.
- **Tar/gzip handling** — destination filename fixed (`downloader.rs:408`), non-`Regular` rejected
  (`:393-399`), declared size pre-checked (`:402-406`), `CappedReader` (`:339-360`) counts decompressed
  bytes across skipped entries (pinned by a test). Extraction only into a fresh 0700 stage (`:316-326`),
  so no pre-planted symlink to follow.
- **Digest bypass (SEC-02 exception check)** — not bypassable, not absent on any path, not weaker than
  documented: `verify_digest` (`downloader.rs:157-179`) treats `Ok(None)` and `Err` as fatal,
  `install_version_dir` has no caller but `download_bb`, ordering intact. No regression.
