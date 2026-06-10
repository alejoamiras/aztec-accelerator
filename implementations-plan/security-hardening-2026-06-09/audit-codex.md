`reject (blocking: PR-1’s authority/port contract is contradictory and not pinned for HTTP/2; PR-4 replaces `tauri-plugin-updater`’s verified download path without proving equivalent verification/transport semantics).`

**Critical**
- `[Inferences] implementations-plan/security-hardening-2026-06-09/plan.md:20,54` vs `packages/sdk/src/lib/accelerator-transport.ts:90-91,125` and `packages/sdk/src/lib/accelerator-prover.ts:141`: PR-1 says “exact-port matching” but line 20 still allows an absent port. For `127.0.0.1:59833/59834`, the real clients send explicit non-default ports, so accepting no-port weakens the invariant for no client benefit. Also the plan does not pin what happens if HTTP/2 `:authority` and `Host` disagree; that must be fail-closed.
- `[Facts] implementations-plan/security-hardening-2026-06-09/plan.md:35-36` vs `tauri-plugin-updater-2.10.1/src/updater.rs:648-719,1453-1462`: PR-4 is the biggest regression risk. In the plugin, signature verification happens inside `Update::download()`, not `install(bytes)`. `install(bytes)` trusts caller-supplied bytes. A repo-owned streaming downloader therefore has to reproduce the plugin’s exact minisign flow and request-builder behavior, or it silently weakens the updater.

**High**
- `[Inferences] implementations-plan/security-hardening-2026-06-09/plan.md:20` vs `packages/accelerator/src-tauri/src/server/tls.rs:63-65`: the plan assumes axum/hyper will hand middleware both HTTP/1.1 `Host` and HTTP/2 `:authority`, but this stack has no regression proving what the app actually sees on the HTTPS/H2 path. PR-1 needs an H2/TLS test and should read `req.uri().authority()`/equivalent, not only `headers().get(HOST)`, or Safari HTTPS can false-403.
- `[Asks] implementations-plan/security-hardening-2026-06-09/plan.md:29,84` vs `packages/sdk/src/lib/accelerator-prover.ts:258-323`: minimal public `/health` does not preserve `needsDownload` correctness. If `available_versions` is absent, the SDK returns `available: true` and `needsDownload: false`. The plan’s proposed unit test only checks `available:true`; it does not validate the broken field the ledger explicitly worries about.
- `[Facts] implementations-plan/security-hardening-2026-06-09/plan.md:55,58`: the ledger says the PR-4 verifier rewrite is “still open for audit”, but line 55 already chooses it. Same problem for SEC-09 symlink swap. Those disputes are not open; they are pre-decided without closure criteria.

**Medium**
- `[Inferences] implementations-plan/security-hardening-2026-06-09/plan.md:20`: host parsing is under-specified. If implemented with ad-hoc string splitting instead of an RFC authority parser, you risk malformed-authority acceptance/rejection bugs: duplicate `Host`, userinfo-bearing authorities, or odd IPv6 forms. “Exact match” is only safe if the input is parsed canonically first.
- `[Facts] implementations-plan/security-hardening-2026-06-09/plan.md:58` vs `packages/accelerator/src-tauri/src/certs.rs:418-427` and `src-tauri/src/main.rs:55-70`: the Windows half of the SEC-09 portability dispute is mis-scoped. This cert/trust path is macOS-only today; Windows symlink perms are not a current blocker.

**Low**
- `[Facts] implementations-plan/security-hardening-2026-06-09/plan.md:87` vs `.github/workflows/_e2e.yml`, `_e2e-app.yml`, `_e2e-webdriver.yml`, `accelerator.yml`: I do not see an unlisted CI break from headless deny-by-default. `_e2e.yml` uses Bun/Node loopback callers, `_e2e-app.yml` and WebDriver do not run the headless server, and release smoke only curls loopback `/health`.

**Looks sound**
- The Host allowlist idea itself is the right keystone against DNS rebinding.
- Real clients today target loopback hosts: SDK default is `127.0.0.1`, landing probes `127.0.0.1`, and WebDriver posts to `127.0.0.1`.
- I do not see a remote-web no-Origin browser path after PR-1; the preserved no-Origin callers are the intended local/Node/CI cases.