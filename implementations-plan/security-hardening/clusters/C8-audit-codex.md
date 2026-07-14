Reject as written. The F-010 serializer does not preserve all accepted paths, accepts paths systemd cannot execute, and misses the adjacent autostart serializers. F-016 is feasible only as a partial mitigation; the fallback proposed in the plan would scrub the wrong copy.

1. F-010 escaping semantics

The proposal at [C8-plan.md:47](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:47) is not correct or complete.

For `ExecStart`, current systemd processes the first item approximately as:

1. Parse the unit line and perform quote/C-escape decoding.
2. Split it into items; item 0 is the executable and also normally `argv[0]`.
3. Strip executable prefixes such as `:`, `@`, `-`, `+`, `!`, `|`.
4. Expand `%` specifiers.
5. Validate the resulting executable path.
6. At execution time, perform `$` environment expansion on the argv array unless disabled.

That ordering is visible in the [upstream `config_parse_exec`](https://github.com/systemd/systemd/blob/main/src/core/load-fragment.c) and later [`replace_env_argv`](https://github.com/systemd/systemd/blob/main/src/core/exec-invoke.c). The documented token/argv rules agree: the first decoded item is the command and later items are arguments. [systemd.service command-line rules](https://man7.org/linux/man-pages/man5/systemd.service.5%40%40systemd.html#COMMAND_LINES), [systemd.syntax quoting rules](https://man7.org/linux/man-pages/man7/systemd.syntax.7.html#QUOTING).

Consequences:

| Input character | Required treatment |
|---|---|
| Space | Safe inside one quoted item. It does not split argv. |
| `%` | `%%` is correct. C-unquoting happens first, then specifier expansion. |
| `$` | Missing from the plan. Quotes do not suppress systemd environment expansion. `${FOO}` in the path can change `argv[0]`. |
| `;` | Safe inside the quoted first item. A separate unquoted `;` is special; this is not a shell command. |
| `&`, `<`, `>`, shell syntax | Ordinary characters without the `|` shell prefix. |
| `"` or `\` | Escaping makes the unit syntactically parsable, but current systemd then rejects the decoded executable path. They are not representable directly. |
| `'`, `*`, `?`, `[` | Also rejected by current systemd executable-path validation; the plan misses them. |
| Controls/newline/DEL | Rejected by systemd; fail closed is correct. |
| Non-UTF-8 bytes | Rejected: current systemd’s executable-path safety check requires valid UTF-8. `\xNN` cannot bypass the post-decode check. |

The path validation comes from `string_is_safe(path, 0)`: valid UTF-8, no controls, backslash, either quote, or glob introducers. See [upstream `string_is_safe`](https://github.com/systemd/systemd/blob/main/src/basic/string-util.c).

There is also an argv0-vs-executable trap. Doubling `$` is not a solution for the executable token: the executable path retained by systemd would contain `$$`, while the later argv expansion could reduce `argv[0]` to `$`. The two diverge.

The clean representation for otherwise supported paths is to use systemd’s `:` prefix to suppress environment expansion, quoted as part of the first item:

```ini
ExecStart=":/absolute path/100%% literal $"
```

After unquoting, `:` is stripped as a prefix, `%%` becomes `%`, and `$` is not expanded. The implementation should nevertheless reject non-absolute, non-UTF-8, control-containing, backslash, quote, and `*?[` paths.

Therefore:

- Operating over raw `OsStr` bytes as proposed at [C8-plan.md:48](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:48) is misleading. The supported contract is valid UTF-8, not arbitrary Linux path bytes.
- “Escape `"`/`\`” must become “reject because systemd rejects the decoded executable.”
- Fail-closed is right because not every Unix pathname is representable in `ExecStart`.
- Failure must be returned to `set_autostart`, not merely logged through the current `fn enable(&self)` API at [crash_recovery.rs:16](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:16).
- Fail-closed must also disable/remove any previously enabled stale unit; simply returning before the write at [crash_recovery.rs:173](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:173) can leave an old unit armed.

2. `systemd-analyze verify`

It is useful but insufficient, and the proposed assertion may be hollow.

`verify` detects parser errors, bad directives, missing dependencies, and nonexistent/non-executable commands. It does not establish that the decoded command equals the intended `current_exe`, nor that a second syntactically valid directive was not injected. A newline injection producing a valid `ExecStartPre=/bin/...` can pass.

Worse, current `systemd-analyze verify` returns success by default even when it emits warnings unless `--recursive-errors=yes|no|one` is supplied. See the [`verify` and `--recursive-errors` documentation](https://man7.org/linux/man-pages/man1/systemd-analyze.1.html). Thus “assert status success” at [C8-plan.md:64](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:64) may prove nothing.

Use it as a compatibility check:

```text
systemd-analyze --user --man=no --recursive-errors=yes verify UNIT
```

Do not skip it silently on the designated Linux CI runner. Separately assert:

- exactly one `ExecStart` item/directive;
- no unexpected line breaks;
- the serializer’s inverse equals the intended UTF-8 path;
- `%`, `$`, spaces, and semicolons preserve exact executable and `argv[0]`;
- every systemd-unrepresentable character is rejected.

Even then, verification is time-of-check syntax/existence validation, not binary identity. Replacement of the path after verification remains possible.

3. F-016 feasibility and residual

`Zeroizing<KeyPair>` does compile once `zeroize` is a direct dependency.

The manifest already enables rcgen’s feature at [Cargo.toml:56](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/Cargo.toml:56), and the lockfile pins rcgen 0.13.2 with `zeroize` at [Cargo.lock:3575](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/Cargo.lock:3575). Rcgen implements `Zeroize for KeyPair` at [rcgen/lib.rs:656](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rcgen-0.13.2/src/lib.rs:656).

But that implementation only does:

```rust
self.serialized_der.zeroize();
```

`KeyPair` separately contains the crypto backend and serialized `Vec` at [rcgen/key_pair.rs:67](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rcgen-0.13.2/src/key_pair.rs:67). The ring backend’s ECDSA private scalar and nonce key are separate fields at [ring/signing.rs:61](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ring-0.17.14/src/ec/suite_b/ecdsa/signing.rs:61), without a zeroizing `Drop`. Ring’s generated PKCS#8 `Document` is another stack-resident copy without zeroizing drop at [ring/pkcs8.rs:178](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ring-0.17.14/src/pkcs8.rs:178).

Accordingly:

- `Zeroizing<KeyPair>` usefully scrubs rcgen’s owned serialized DER allocation.
- It does not scrub ring’s private scalar/nonce state or generation temporaries.
- Rcgen’s `zeroize` feature does not scrub anything automatically on the current plain drop at [certs.rs:151](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:151); the trait must be invoked or wrapped.
- A newtype around `ca_key.serialize_der()` only wipes a clone. It leaves rcgen’s original `serialized_der`, backend state, and prior temporaries untouched. That fallback should be deleted from [C8-plan.md:42](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:42).

A direct `zeroize = "1"` dependency is needed to name `Zeroizing`; transitive dependencies are not directly importable. Version 1.8.2 is already locked at [Cargo.lock:6649](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/Cargo.lock:6649). The repository’s seven-day rule is explicitly a Bun/npm resolution rule at [bunfig.toml:7](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/bunfig.toml:7); it does not gate Cargo. Adding this already-locked crate should not fetch a new package version.

Explicit early drop after [certs.rs:146](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:146) is still meaningful: it wipes rcgen’s DER before potentially blocking/failing file writes and shortens the live signing-object lifetime. It does not erase an earlier swap snapshot, core dump, backend copy, compiler spill, or allocator history. The residual is not tightly bounded; call this “best-effort post-use recovery reduction,” not “the only copy is gone.”

Also, [C8-plan.md:73](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:73) says the cert set is “byte-identical.” It cannot be: key generation is randomized. Only validity, chaining, file set, and behavior are unchanged.

4. Testability and validation gate

A GUI display is not required to run these library/unit tests. Building `src-tauri` does require GTK/WebKit development libraries.

The real Linux CI installs those dependencies at [setup-accelerator/action.yml:79](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/.github/actions/setup-accelerator/action.yml:79) and runs `cargo test` without Xvfb at [accelerator.yml:88](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/.github/workflows/accelerator.yml:88). A GUI-less VPS can therefore run it if those build libraries are installed.

Do not move systemd policy into `accelerator-core` merely to avoid local Tauri dependencies. The helper can remain a pure function in the existing library module exposed at [lib.rs:12](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/lib.rs:12). If a developer machine lacks packages, CI is the required gate—not an “extract OR CI” escape hatch.

The plan’s presence-gated `systemd-analyze` test and the pseudo-command at [C8-plan.md:72](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:72) are not real blocking gates. Require the analyzer on Linux CI and spell out executable commands.

5. Facts / inferences / asks that need correction

- “Rcgen may not implement Zeroize” at [C8-plan.md:31](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/implementations-plan/security-hardening/clusters/C8-plan.md:31) is false for this locked build.
- “Rcgen internally scrubs serialized DER” is incomplete: it only exposes a `Zeroize` implementation; current plain drop does not invoke it.
- “Only copy” at [certs.rs:151](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:151) is false.
- Failing only on controls is insufficient; systemd’s accepted executable-path set is narrower.
- A warning-only `enable()` is not fail-closed from the caller’s perspective. [commands.rs:52](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/commands.rs:52) returns success after the autostart manager succeeds even if crash recovery silently fails.
- The plan needs an explicit supported-systemd-version assumption because prefix and parser behavior must be compatible with the oldest supported Linux distribution.
- The owner needs to choose whether F-016 is an accepted partial mitigation or requires backend-level zeroization. The current plan silently treats partial scrubbing as closure.

6. Threat model, blast radius, and nearby surfaces

F-010 is user-level persistence, not privilege escalation. The unit is under the user manager and commands run as that user. An attacker already able to arbitrarily write the victim’s `~/.config/systemd/user` or replace a user-writable executable has equivalent or stronger power. The meaningful scenario is a crafted distribution/install pathname that a victim launches and then enables for autostart. Injection becomes persistent code execution on the next user-manager/default-target activation; `enable` itself does not start the unit.

More importantly, the same root cause already exists in the autostart layer executed immediately before crash recovery:

- Linux `auto-launch` emits raw, unquoted `Exec={} {}` at [auto-launch/linux.rs:33](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/linux.rs:33). Spaces, field codes, and newlines remain unsafe.
- macOS `auto-launch` embeds path and arguments into raw XML `<string>` elements without XML escaping at [auto-launch/macos.rs:87](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/macos.rs:87). The local plist patch only adds constants; it inherits that malformed/injected plist.
- The plugin converts `current_exe` through lossy display strings on all platforms at [tauri-plugin-autostart/lib.rs:186](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-plugin-autostart-2.5.1/src/lib.rs:186).
- Windows `auto-launch` writes an unquoted Run-key command line at [auto-launch/windows.rs:37](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/windows.rs:37), creating the classic spaced-path ambiguity.
- The repository’s own Windows Task Scheduler XML is substantially better: the executable is a structured `<Command>` at [crash_recovery.rs:384](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:384) and XML entities are escaped at [crash_recovery.rs:393](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:393). Residuals are lossy UTF-16 conversion at [crash_recovery.rs:270](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:270) and XML-invalid controls, not ordinary quote/space injection.

Fixing only the systemd unit therefore does not close the crafted-path persistence primitive. The plugin serializers must be patched, replaced, or preflight-rejected in the same scope.

F-016 requires process-memory, freed-memory, core-dump, swap, or forensic access. The CA is constrained at [certs.rs:97](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:97) to localhost and loopback names, assuming trust consumers enforce root constraints. Recovery permits minting fresh trusted localhost certificates until the ten-year anchor expires, but it is not a general Internet MITM key. Because the leaf key is already intentionally persisted at [certs.rs:150](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:150), the CA key’s incremental value is longer-lived minting authority rather than immediate access to an otherwise unavailable localhost identity.

Nearby secret residuals worth recording, though not necessarily blocking this cluster:

- Windows leaf-key creation has no explicit ACL hardening at [certs.rs:242](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:242).
- `load_rustls_config` creates additional plain `Vec`/decoded leaf-key copies at [certs.rs:260](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:260), although the key remains live by design.
- Legacy `ca.key` removal at [certs.rs:195](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:195) does not erase filesystem snapshots, SSD remnants, or backups.

7. Competing outline

For F-010:

- Preflight the executable before invoking `manager.enable`, so the third-party `.desktop`/plist/Run-key writers cannot serialize an unsafe path first.
- For systemd, support only absolute valid-UTF-8 paths accepted by systemd; use a single quoted token with the `:` prefix, double `%`, and reject the post-decode forbidden set.
- Return a real error, remove any stale unit, and write the unit atomically.
- Require exact serializer/inverse tests plus mandatory `systemd-analyze --recursive-errors=yes`; treat analyzer output as syntax compatibility only.
- If exotic Unix paths must work, do not serialize the real target. Create an atomically managed symlink at a fixed safe launcher path and keep the unit static. That lets the kernel follow arbitrary symlink target bytes without placing those bytes in unit syntax.
- A persistent unit file has no text-free “exec array” form. D-Bus transient units have structured argv, but they are not a drop-in replacement for an enabled persistent service.

For F-016, choose an assurance level:

- Minimal honest mitigation: direct `zeroize` dependency, `Zeroizing<KeyPair>`, immediate `drop` after signing, and document that only rcgen’s serialized DER is guaranteed wiped.
- Higher assurance: use `rcgen::RemoteKeyPair` backed by a signer with genuine `ZeroizeOnDrop`, or a native non-exportable Keychain signer. This avoids rcgen owning a serialized CA-key copy and gives the backend a defensible destruction contract.
- A self-signed persistent leaf removes the distinct CA key, but only adopt it after proving macOS/browser direct-leaf trust semantics and confirming the persistent leaf key cannot become an unconstrained trust-anchor signing key.

VERDICT: reject (incomplete F-010 serialization and platform scope; ineffective F-016 fallback and overstated residual; non-enforcing validation gates)