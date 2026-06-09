clean

**Findings**

No findings. I did not find a behavior regression in F-07, F-09, or F-08.

**Checks**

- F-07 is source-equivalent where it matters. `write_new_cert_set` still writes the same three files in the same order, `ca.pem` then `localhost.pem` then `localhost.key` (`packages/accelerator/src-tauri/src/certs.rs:145-147`). Rotation still swaps staged files into live in the same `ca -> leaf -> key` order via `swap_into` (`certs.rs:67-72`, `certs.rs:313-315`). Trust-failure cleanup still removes the same three staged files (`certs.rs:60-65`, `certs.rs:308-310`). `certs_exist` still requires all three live files and still keeps the leaf-validity gate through `leaf_cert_days_remaining() > 0`, so this did not collapse into a pure existence check (`certs.rs:130-131`, `certs.rs:258-267`). `install_ca_trust` and `is_ca_trusted` still target the live `ca.pem` path (`certs.rs:403-412`). I also checked the PR head for stale uses of the removed served-path helpers and found none.

- F-09 is a true wrapper extraction. The new helper is still exactly the old `spawn(async move { if let Err(e) = start_https(...) { tracing::error!(...) }})` pattern, just centralized in `packages/accelerator/src-tauri/src/server.rs:15-23`. The caller-specific behavior stayed upstream: launch-time `try_start_https` still resets Safari Support if TLS config load fails before spawning (`packages/accelerator/src-tauri/src/main.rs:72-85`), while settings-time `enable_safari_support` still propagates TLS-load failure back to the command caller and only spawns on success (`packages/accelerator/src-tauri/src/commands.rs:161-175`).

- F-08 also preserves the observable prove flow. `prove()` still emits the leading `Proving` before any version resolution (`packages/accelerator/core/src/server/prove.rs:146-151`), so the no-download path remains `[Proving, Idle]` via the existing drop guard. The download path now emits `Downloading`, runs the same `download_bb` call, spawns the same `cleanup_old_versions` task, returns the same `download_failed` payload on failure, and re-emits `Proving` before `bb::prove`, preserving `[Proving, Downloading, Proving, Idle]` (`prove.rs:160-195`). The bundled-version recompute is equivalent because `bundled_version` is immutable state, not lock-backed mutable config (`packages/accelerator/core/src/server.rs:80-108`). The cache/download predicate is also unchanged: it is still `requested != bundled && !version_bb_path(&version).exists()` (`prove.rs:74-84` vs base). `AztecVersion` display is raw-string-preserving, so switching `{v}` to `{version}` does not change the error text (`packages/accelerator/core/src/versions/mod.rs:80-132`).

**Residual risk**

The bundled/no-download status sequence is characterization-tested, but the four-event download arm is still only source-verified, not directly exercised by a deterministic test (`packages/accelerator/core/src/server.rs:691-746`, `server.rs:1107-1121`).