Verdict: **reject** — blocking: outer-feed binding was dropped, raw Tauri updater commands bypass `VerifiedUpdate`, and the floor/install design is not cross-process monotonic.

1. Base64-verbatim

Base64-verbatim is the correct encoding decision: decoded bytes are exactly those signed, with no JSON reserialization drift. But the consolidation lost Codex’s semantic outer binding.

[C4-CONSOLIDATED.md:28](implementations-plan/security-hardening/clusters/C4-CONSOLIDATED.md:28) only requires SemVer equality plus any matching URL/signature. Restore the requirements from [C4-plan-codex.md:68](implementations-plan/security-hardening/clusters/C4-plan-codex.md:68) and [C4-plan-codex.md:73](implementations-plan/security-hardening/clusters/C4-plan-codex.md:73):

- Strictly typed outer `version`, `pub_date`, and entire `platforms` projection must equal the decoded signed envelope.
- Require exact canonical `Update.version == envelope.version`, not merely semantic equality.
- `notes` may remain unsigned.
- Explicitly define `manifest_sig` as Tauri `.sig` base64, decode it once, and cap its length too.

Selected URL/signature/size alone blocks the core rollback splice, but permits unsigned platform permutation and `pub_date` divergence and contradicts the promised date/platform tamper tests.

2. VerifiedUpdate / TOCTOU

Native paths can be sealed by changing `PendingUpdate` to `Option<VerifiedUpdate>` and keeping the constructor/inner `Update` private. Today the two native sinks are [updater.rs:47](packages/accelerator/src-tauri/src/updater.rs:47) and [commands.rs:243](packages/accelerator/src-tauri/src/commands.rs:243).

However, [capabilities/default.json:6](packages/accelerator/src-tauri/capabilities/default.json:6) grants `updater:default`, exposing raw frontend `check`, `download`, `install`, and `download_and_install`. That completely bypasses the newtype. Remove those permissions or explicitly deny all updater plugin commands; add a denial test.

Rechecking the floor only at `perform_update` entry is also insufficient across a long download.

3. Monotonic floor

The state semantics are otherwise sound: missing bootstrap, corrupt/I/O fail-closed, no corrupt overwrite, running below floor disabled, and RC ordering via SemVer.

Blocking gaps:

- [C4-CONSOLIDATED.md:32](implementations-plan/security-hardening/clusters/C4-CONSOLIDATED.md:32) specifies only a process mutex. Multiple instances can race signed N/N+1 installs and floor commits. Current server startup and updater poller are independent tasks: [main.rs:223](packages/accelerator/src-tauri/src/main.rs:223), [main.rs:263](packages/accelerator/src-tauri/src/main.rs:263).
- Require an OS-wide updater transaction plus a monotonic pending/accepted version, or prove only the successful HTTP-bind owner can check/install/commit. Recheck under that transaction immediately before install.
- “HTTP spawned” is not bind success. The floor tracker must receive actual bind success.
- Three rapid probes are not stricter than Fable’s 30-second grace. Specify spacing, minimum dwell, and reset-on-failure.
- Use `Version::cmp_precedence`; restore the source plan’s build-metadata-only rejection test. Ordinary `semver::Version` ordering includes build metadata.

Crash recovery then safely relaunches the installed build; the health commit merges pending/current into the floor without allowing another instance to install a lower candidate first.

4. Signing-job isolation

The intended separation is correct: signing/build jobs may have `contents:read` plus artifact-store upload, while `tag`/`release` hold repository/OIDC write without the private key. Current privileged release permissions are [release-accelerator.yml:566](.github/workflows/release-accelerator.yml:566).

Required clarifications:

- Scope signing secrets to the single signer step; use the lockfile-pinned local Tauri CLI.
- Add `sign-update-feed` directly to release smokes and `verify-live-feed.needs`, so its SHA output is available.
- RC feeds remain internal artifacts; only stable copies `latest.json` into release files.
- The Rust example assembles/verifies; Tauri CLI signs. [C4-CONSOLIDATED.md:22](implementations-plan/security-hardening/clusters/C4-CONSOLIDATED.md:22) incorrectly says the Rust tool signs.

5. E2E

The fixture-key Linux test plus a current-code Windows client using the real signed feed can prove enforcement day one. Real macOS/Linux N−1 positives prove only backward compatibility.

Fixes:

- “Tampered artifact (valid sig)” is wrong. Flip a byte in place, preserving length and the genuine original signature; otherwise the new size check could reject and falsely appear to prove plugin signature enforcement.
- The Windows throwaway key is only signing unused N−1 updater artifacts; it does not alter the embedded production pubkey. Clean fix: temporarily set `createUpdaterArtifacts=false` for synthetic N−1 and keep the production pubkey. Patching N−1 to the throwaway pubkey would break the production-signed N test.
- Assert splice rejection has a `SECURITY:` log and zero artifact requests.

6. Residuals

SEC-03 is mostly framed correctly, but [C4-CONSOLIDATED.md:39](implementations-plan/security-hardening/clusters/C4-CONSOLIDATED.md:39) should say a compromised artifact origin/CDN or TLS terminator—not an ordinary network-position attacker—can stream unbounded bytes. The post-download size check does not mitigate buffering DoS.

Also, “all replay fails closed” contradicts the admitted intermediate-feed replay residual. Only replay at or below current/floor fails.

7. Assumptions

- “Successful launch signal” is an inference, not a fact.
- `config.rs` is only a starting pattern: it uses a predictable temp path, ignores chmod failure, and lacks fsync at [config.rs:146](packages/accelerator/core/src/config.rs:146). Security-state implementation must retain the plan’s stronger semantics.
- Signing-secret availability is a blocking release prerequisite, not a non-blocking ask.
- F-005 is independent and must not gate C4.
- The old-key bridge rotation story strands clients that miss the bridge; restore Fable’s second-channel/dynamic migration requirement before claiming rotation is solved.