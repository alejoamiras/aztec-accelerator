# c4-updater-trust — Claude (Opus) raw findings

One finding. Layer A is verified complete against a fully hostile feed (documented below as a
load-bearing negative). The finding is in Layer B's persistence + the lock.

## F-C4-1 — One unauthenticated local write permanently and silently disables the auto-update channel, and survives uninstall + reinstall

**Impact.** Availability of the security-patch channel → cascading Integrity: the app is pinned forever
to its current build, so every FUTURE vulnerability in a component that terminates TLS for a
browser-reachable listener, handles private witnesses, and installs a CA into OS trust stores stays
exploitable on that machine. Blast radius: one machine per write, but **permanent and irreversible by the
normal remediation** (reinstall does not clear it). One variant also disables the autostart self-heal as
collateral. Vector local; complexity LOW (~40 bytes, or one `chmod 000`); privileges LOW — any code
running as the user, ONE SHOT, no persistence needed; no user interaction. Also reachable **accidentally**
with no attacker (variant C).
**Confidence** high on mechanism and permanence (every branch explicit in-source; no repair path exists
anywhere in the repo); moderate on severity class, because the precondition is a same-user local actor —
which this app's own threat model explicitly admits as hostile.
**CWE-15** (primary), CWE-1329 (consequence), CWE-755 (contributing). OWASP A08:2021.
Honest note from the agent: none are in the current Top 25; the nearest neighbour CWE-732 does NOT apply —
the file is correctly `0600` in a `0700` dir (`updater_state.rs:205-245`). The exposure is same-user.

**Variant A — poisoned-but-VALID floor (irreversible by design).**
Write `{"schema":1,"floor":"999.0.0"}` to `~/.aztec-accelerator/updater-state.json` (path built
`updater.rs:34-36`) → parses cleanly to `LoadedState::Valid` (`updater_state.rs:75-99`) — a LEGITIMATE
state, not corruption; nothing rejects it → `layer_b_gate` (`updater.rs:136-142`) →
`running_below_floor` returns true (`updater_state.rs:140-142`) → `Err("running build is BELOW the version
floor … refusing all updates")` → `updater.rs:184-187` logs and returns `None`. Every update, from every
version, forever. Irreversible: `commit_successful_launch` (`updater_state.rs:189-198`) and
`record_pending` (`:162-173`) are monotonic-MAX and never lower `floor`.

**Variant B — corrupt state.** One garbage byte (or a directory, or `chmod 000`) → `Corrupt` via any of
`updater_state.rs:76-80` (non-NotFound IO), `:81-84` (parse), `:85-87` (schema), `:88-90` (non-canonical
floor), `:92-98` (pending) → `:122-123` `Corrupt => false` rejects EVERY candidate → `updater.rs:143-147`
→ refuse. Irreversible: both mutators (`updater_state.rs:154-159`, `:183-187`) return InvalidData
("refusing to overwrite a corrupt version-floor state"). Nothing repairs or deletes it.

**Variant C — lock-file denial (widest collateral).** `chmod 000 ~/.aztec-accelerator/updater.lock` or
replace with a directory → `updater.rs:51-57` `OpenOptions::…open(…).ok()?` → `None` on EACCES/EISDIR.
This is the OPEN failing, so the `try_lock_exclusive` fallback (`:58-66`) is never reached and **nothing is
logged**. Silent sinks: `perform_update` returns immediately (`:275-278`) — no install ever;
`commit_launch_floor` defers forever (`:91-96`) — floor never advances, stale `pending` never clears;
`heal_if_broken` returns `Skipped("updater active")` forever (`autostart.rs:1365-1367`) — **the autostart
self-heal is permanently disabled as a free side effect**.

**Persistence across the standard remediation.** The only uninstall cleanup is `nsis/hooks.nsi:88-113`,
which removes the CA and `RMDir /r "$PROFILE\.aztec-accelerator\certs"` — **only `certs/`**.
`updater-state.json` and `updater.lock` are untouched; macOS/Linux have no uninstall hook at all. Repo-wide
grep: no code path anywhere removes, repairs, or resets either file.

**Missing control.** (a) No integrity binding on the state file — a security decision input with no
MAC/signature and no binding to the installation; its only defence is permissions, which are irrelevant to
the same-user threat. (b) No bounded/recoverable fail-closed policy: `Corrupt` means "disable updates
forever" rather than "fall back to the provably safe floor". A strictly stronger policy costs nothing —
on `Corrupt` (and on `running_below_floor`) use `floor = current_running_version`, which preserves the
entire anti-rollback property that matters, because `candidate_allowed` ALREADY enforces
`candidate > current` unconditionally (`updater_state.rs:119-121`) independent of the state file. The only
thing the persisted floor adds over `current` is protection against an out-of-band downgrade of the binary
itself, which requires an attacker who already owns the app files. (c) No self-heal/quarantine arm.
(d) No user-visible signal — every path is `tracing::error!`/`warn!` to a log file; the prompt simply never
appears again, and a user cannot distinguish "no update available" from "updates permanently disabled".
(e) `.ok()?` at `updater.rs:57` collapses "another instance holds it" (benign, logged `:60-65`) and "lock
file unopenable" (permanent, unlogged) into the same silent `None`.

**Exploit story.** One-shot unprivileged execution as the user (malicious transitive dependency
postinstall, a helper the user runs once) → one line:
`printf '{"schema":1,"floor":"999.0.0"}' > ~/.aztec-accelerator/updater-state.json` → foothold removed,
wedge remains → every later launch refuses every candidate; the prompt never appears again → vendor ships
a critical fix; this machine never receives it → user/IT uninstalls and reinstalls; `hooks.nsi:111` removes
only `certs/`; the wedge survives and the fresh build is equally cut off.
**Variant C accidental trigger:** a user who deliberately downgrades to an older build from GitHub Releases
(normal when a release is bad) trips `running_below_floor` and is permanently cut off from ALL future
updates — including the fix that would restore them — with the same silence and no recovery path.

**Preconditions.** Any code execution as the user, once. No elevation, persistence, network position, or
user interaction. Variant A needs nothing beyond the write (the file is schema-valid). Requires a real
installed instance with auto-update enabled (shipped release config; `main.rs:216-228`).

**Why mitigations fail.** 0600/0700 are CROSS-USER controls, irrelevant here. Atomic write + fsync +
dir-fsync protects OUR writes from tearing; it does not authenticate the file, and in variant A the
attacker's file is indistinguishable from ours. `deny_unknown_fields` + canonical-SemVer round-trip
(`updater_state.rs:41,66-71`) INCREASE the surface — they turn more inputs into `Corrupt`, i.e. into
permanent lockouts — and variant A bypasses them with a perfectly canonical document. The "fail closed
preserves forensic evidence" intent (module doc `:21-22`) is sound but the UNBOUNDED consequence is the
gap: a corrupt state file is indistinguishable from disk corruption or a truncated restore, and the
`Corrupt` policy discards the one anti-rollback guarantee that does not depend on the file
(`candidate > current`) — giving up availability without buying integrity it did not already have.
The `updater.lock` is the mechanism variant C attacks, not a defence. Layer A is orthogonal (hostile FEED,
not hostile local FILE).

**Instances.** `updater_state.rs:75-80,81-84,85-87,88-90,92-98` (Corrupt mints), `:122-123` (reject all),
`:140-142` (variant-A trigger), `:154-159,183-187` (refuse to repair), `:162-173,189-198` (monotonic max ⇒
irreversible); `updater.rs:34-36` (unauthenticated path), `:51-57` (silent lock-open failure),
`:91-96` (floor commit deferred), `:132-149` (gate sink), `:184-187,304-310,392-408` (log-only consumers);
`autostart.rs:1365-1367` (collateral); `nsis/hooks.nsi:88-113` (wedge survives reinstall).

## VERIFIED CLEAN (deliberate negative results — do not re-audit)

**Layer A is complete against a fully hostile feed without the signing key.** Modelled a feed-writer
controlling every byte of `latest.json`, checked field-by-field against the actual
`tauri-plugin-updater-2.10.1/src/updater.rs`:
- Every feed field the plugin CONSUMES is bound: `version` (`update_manifest.rs:181-184,200-202`),
  `pub_date` (`:185-188`), the entire `platforms` map incl. keys (`:189-197`, BTreeMap equality with
  `deny_unknown_fields` on `PlatformEntry`), the selected artifact's `signature` (`:216-218`) and `size`
  (`:222`, from the SIGNED envelope). The only unbound field is `notes` → `Update.body`, and the app never
  reads `body`/`notes` anywhere (grep-confirmed) — no unsigned-string-into-UI path.
- Rollback/replay: an old validly-signed envelope is stopped twice — the plugin's own
  `release.version > current_version` (plugin `:534`) and unconditionally by `candidate > current`
  (`updater_state.rs:119-121`), which does not depend on the state file.
- Envelope/feed mixing: outer↔envelope equality is total over `{version, pub_date, platforms}`.
- URL redirection: `platforms[*].url` is byte-bound; `url_matches` (`:107-112`) only normalizes
  percent-encoding and falls back to STRICTER string equality when either side is unparseable.
- Ambiguous/duplicate platform keys: plugin and Layer A read the SAME `serde_json::Value` (plugin
  `:492-493`), so duplicate-key collapse cannot diverge. `platforms: null` → plugin falls through to the
  Dynamic shape while Layer A rejects (`:192-194`) — fail-closed. `platforms: {}` → plugin errors first.
- Version smuggling: the plugin strips leading `v`s; Layer A's canonical round-trip (`:173-177`) plus the
  raw-string outer comparison rejects `v1.0.9`/`vvv1.0.9`.
- Domain separation: the artifact signature is over binary bundle bytes; the envelope signature requires
  bytes parsing as strict JSON with `schema == "aztec-accelerator-update-manifest-v1"`.
- Signed-vs-served confusion: `download()` verifies the artifact signature over the buffered bytes BEFORE
  returning (plugin `:712`) and `perform_update` passes those exact bytes to `install()`
  (`updater.rs:338-356,533`) — no disk round-trip, no verify→install TOCTOU. The
  `bytes.len() == signed_size` equality (`:362-369`) is enforced and the cap (`:326-333`) reads the SIGNED size.

**The pinned pubkey cannot fail open or diverge.** Exactly one `tauri.conf.json` (no per-platform
overrides), and the release pipeline patches that FILE ON DISK (`release-accelerator.yml:175-186`) rather
than `tauri build --config`, so `include_str!` (`updater.rs:21`) sees byte-identical content to what
`tauri-build` embeds. Absent key panics at build time; empty key fails closed at `PublicKey::decode`.
Endpoint is HTTPS; `dangerous_accept_invalid_certs` not set.

**`VerifiedUpdate` is genuinely unforgeable.** Private fields, no `Deserialize`/`Clone`/`Default`, single
private constructor, only accessor `version()`. No capability file grants `updater:default` or
`core:default`, and `withGlobalTauri` is false — the webview cannot reach the plugin's JS commands.

**Lock ordering sound.** Both nesting sites take `updater.lock` → `autostart.lock` in the same order
(`updater.rs:414-421`, `autostart.rs:1365-1379`); no inversion. Same-process re-entry correctly bows out
(flock/LockFileEx are per-file-description). Rust sets `O_CLOEXEC`, so `exec()`-based `app.restart()`
releases the lock rather than leaking it.

**Duplicate-URL platform entries** would make `ArtifactNotUniquelyMatched` (`update_manifest.rs:206-215`)
reject every update — a latent self-DoS footgun — but not a finding: the release pipeline's
`sign-update-feed` job runs the production verifier over EVERY platform entry before publishing
(`core/examples/update-manifest.rs:80-115`, wired at `release-accelerator.yml:762`), so such a feed fails
the release rather than shipping.
