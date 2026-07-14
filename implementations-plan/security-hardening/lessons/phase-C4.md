# C4 — F-004 updater rollback (deep). Lessons / debug log

Branch `sechard/updater-rollback` off `security-hardening`. Cluster tier: **deep** (3 parallel plans +
double audit; GATE-1 artifacts in `clusters/C4-*`). Two-layer defense:

- **Layer A** (`core/src/update_manifest.rs`): signed-manifest envelope binds the advertised version to
  the exact signed artifact set. Closes the splice (feed advertises high version, points at an old
  still-signed artifact).
- **Layer B** (`core/src/updater_state.rs`): monotonic version floor — the rollback ratchet.

## Implementation phases (GATE 2)

| Phase | Commit | What |
|---|---|---|
| 0 | 51feb41 | Layer A verifier `verify_manifest` + 9 tests vs a real tauri-signed fixture |
| 2 | 8b62421 | Layer B floor (`load_floor`/`candidate_allowed`/`commit_successful_launch`) + 8 tests |
| 1a | 940af2f | B1 fix: deny the frontend `updater:default` capability (no webview bypass of the newtype) |
| 1b | db48b4a | Wire A+B into the updater flow via `VerifiedUpdate` proof-carrying newtype; signed-size cap |
| 1c | 7c94b21 | Post-launch floor commit (3 healthy `/health` probes) + `fs2` cross-process lock (B2) + TOCTOU re-check |
| 3a | 9798fc4 | `update-manifest` example tool (envelope/splice/verify) + `build_signed_envelope` in core; 2 tests |

Local validation each phase: `cargo check` + `cargo clippy -D warnings` clean, 25 src-tauri tests + 11
update_manifest tests green. src-tauri compiles locally on this GUI-less box (GTK/webkit dev libs present);
the Tauri `build.rs` needs a stub `binaries/bb-x86_64-unknown-linux-gnu` (gitignored) to pass its
externalBin resource check — created locally, never committed.

## Key design decisions (folded from the GATE-1 deep blueprint + consolidated-plan audit)

- **Envelope = base64-VERBATIM bytes**, not typed-canonical re-serialization. `raw_json` is parsed, not
  bytes; the base64 string survives the plugin's JSON parse intact, so there is zero canonicalization
  drift. `build_signed_envelope` produces the exact bytes the pipeline signs; the SAME `SignedEnvelope`
  struct produces and consumes → no shape drift.
- **Floor lives in a dedicated file**, NOT `config.json` (whose load is fail-OPEN and would silently
  erase the floor). Corrupt floor fails CLOSED and is never overwritten (forensics).
- **`VerifiedUpdate` newtype**: private fields, sole constructor is the verify path; `perform_update` +
  `PendingUpdate` accept only it. With the capability denial (B1) there is no other install route.
- **`manifest_sig` encoding** (confirmed empirically from the committed fixture, since two Codex consults
  died before answering — see below): `base64-decode(manifest_sig)` == the minisign signature document
  (`untrusted comment: signature from tauri secret key\n<sig>\ntrusted comment: … file:envelope…`). So
  `manifest_sig` == the tauri `.sig` file content VERBATIM (tauri already writes `.sig` as
  base64(minisign doc), the same encoding the artifact `signature` fields use). The `splice` subcommand
  therefore embeds the `.sig` content unchanged — re-encoding would double-encode and fail verification.

## Codex consults (GATE-3-adjacent design consult) — BOTH FAILED (environment instability)

Per AFK protocol: codex is advisory; when it dies mid-run, log it and continue on own judgment within
plan scope. Logged here, not silently skipped.

1. **02:36 UTC, session 019f5e7b** (`gpt-5.6-sol` xhigh, Phase 3b/4 design): codex spent the turn
   web-searching the tauri-cli `updater_signature.rs` source to confirm the `.sig` encoding; one fetch
   returned `Internal Error ()` and the run ended after only the preamble `agent_message` (output_tokens
   ≈ 2129, no answer). The `-o` file was never written.
2. **02:39 UTC, session 019f5e7e** (focused, no-web-search prompt): ended after a SINGLE `reasoning`
   entry — no tool calls, no `agent_message`, exit 0. Produced nothing.

Both `-o` output files were absent; recovered transcripts from `~/.codex/sessions/2026/07/14/*.jsonl`.
Conclusion: codex is currently unstable in this environment (likely the web-search backend + an early
end-of-turn). **Proceeded on own judgment**, which is well-grounded because:
- Q1 (job topology) was ALREADY decided by the double-audited deep blueprint: a SEPARATE least-privilege
  `sign-update-feed` job (contents: read + signing secret only), NOT inline in the `contents:write` +
  `id-token:write` (OIDC→AWS) release job. Re-opening it toward "inline is simpler" would contradict an
  approved plan; the blueprint's blast-radius argument (don't co-locate the signing key with prod-cloud
  creds) stands.
- Q2 (encoding) is resolved empirically above.
- Q1 factoring that avoids duplicating the flatten logic: the sign job reads per-platform `.sig` + byte
  SIZE from the ORIGINAL artifact paths (it does NOT flatten; bytes are identical to the renamed release
  files) and builds URLs from VERSION+RELEASE_TAG strings, then generates→envelope→sign→splice→verify and
  uploads the signed `latest.json` as an artifact. The `release` job deletes its jq block and downloads
  that artifact; it never gets the signing key. `release` `needs: sign-update-feed`, so verify (which
  fails the sign job on a bad feed) gates publication of BOTH the GH asset and the S3 copy.

## GATE 3 — post-impl Codex audit (session 019f5e8b, gpt-5.6-sol xhigh) — SUCCEEDED

This run stayed up and surfaced two real findings (folded before merge):

- **F-C4-A (must-fix, release-breaking ripple): the updater-smoke harnesses synthesize an UNSIGNED,
  SIZELESS feed.** `updater-smoke.sh:110`, `updater-smoke-linux.sh:142`, `updater-smoke-windows.ps1:130`
  build `latest.json` as `{version, notes, pub_date, platforms:{key:{signature, url}}}` — no `manifest`,
  no `manifest_sig`, no `size`. A client built from THIS branch now runs Layer A and rejects that feed
  (`MissingField("manifest")`) BEFORE downloading → the POSITIVE update-smoke never updates (fails), and
  the NEGATIVE smoke passes for the wrong reason (rejected by Layer A, not by artifact-sig mismatch — it
  loses its teeth). These smokes run ONLY in `release-accelerator.yml` (via `_e2e-updater{,-linux,
  -windows}.yml`), NOT the PR gate, so PR #387 CI stays green — but a real release would break. Phase 5
  fix: each smoke must (1) add `size` to the platform entry, (2) sign the manifest with the key whose
  pubkey the N-1 build embeds — Windows already generates an ephemeral `n1.key` (self-contained, sign
  with it); macOS/Linux embed the PROD pubkey and the smoke is deliberately keyless ("needs no signing
  key"), so they need `TAURI_SIGNING_PRIVATE_KEY` passed via the reusable-workflow `secrets:` to sign the
  manifest for the LOCAL feed URLs — then envelope→sign→splice via the update-manifest tool. Release-only
  testable (actionlint + shellcheck locally; real validation is a release / workflow_dispatch run).

- **F-C4-B (narrow race, likely document-as-residual): the version-floor advances only AFTER 3 healthy
  probes, so between a successful install→`app.restart()` (which releases the flock at process exit) and
  the new build committing floor=N, a concurrently-racing OLD instance could install a validly-signed
  candidate N' with floor<N'<N — a downgrade from N.** Requires two instances racing in the restart
  window (the `:59833` single-instance bind guard largely prevents this), feed control, and tight timing.
  The 3-probe delay is deliberate (committing floor=N pre-restart would let a crash-looping bad update
  ratchet the floor and brick the updater). Assess severity from Codex's full writeup; if kept, document
  as a residual with a code comment rather than reintroduce the bad-update-bricks-floor problem.

C4 is therefore NOT merge-ready until F-C4-A is fixed (Phase 5) — the GATE-6 "audit clean" bar is not met
while a Codex-flagged release-breaking ripple is open. PR #387's own CI is green (smokes aren't on the PR
gate), but merging as-is would plant a latent release breaker.

### Fold outcome (all 8 findings addressed)

- **H1 restart race** → `updater_state` gains a `pending` high-water: `perform_update` calls
  `record_pending(current, candidate)` UNDER THE LOCK right after `install()`, before the restart
  releases it. `candidate_allowed` now gates on `max(current, floor, pending)`; a racing instance sees
  the raised effective floor and can't install a lower still-signed version. `commit_successful_launch`
  promotes pending→floor once the launched version catches up. (652c439)
- **H2** → `commit_launch_floor` REQUIRES the lock (defers if held), was best-effort. (652c439)
- **H3 tracker commits a never-run version** → confirmed: the redundant-instance bow-out is
  `cfg!(target_os="windows")` ONLY (main.rs:239), so on mac/linux a second instance keeps running. Added
  `healthy_aztec_version_on_port()`; the tracker requires `/health.version == CARGO_PKG_VERSION` ×3. The
  desktop injects its version into `/health` (main.rs:404 `HeadlessState::headless(env!(CARGO_PKG_VERSION))`),
  so the match holds. (652c439)
- **M5** → one `layer_b_gate(candidate, current)` shared by check + install; fail-closed on every arm
  (was fail-open `if let` + omitted `running_below_floor` at install). (652c439)
- **L8** → `write_state` propagates the parent-dir open + `sync_all` errors (were swallowed). (652c439)
- **M6** → documented the feed-response buffer DoS residual at `updater.check()` (plugin buffers the
  whole feed before the manifest cap applies; needs an upstream cap). (7ecbfc1)
- **M7** → `verify-live-feed` now runs the production `update-manifest verify` over the live CDN feed
  (checkout + rust added); catches a corrupted/dropped `manifest_sig` that leaves version/URLs intact. (7ecbfc1)
- **H4** → the updater-smoke feeds are now SIGNED + carry `size`. A shared `sign-smoke-feed.sh`
  (jq-assembled envelope → `bunx tauri signer sign` → base64 splice) is used by the mac + linux smokes;
  the Windows smoke does the equivalent in pwsh. The manifest is signed with the PROD key (the synthetic
  N-1 keeps the committed prod pubkey, verified against L95-97 of `_e2e-updater-windows.yml`), plumbed via
  new `secrets:` on the 3 reusable workflows + the 4 `release-accelerator.yml` calls; on Windows the prod
  key OVERRIDES the ephemeral GITHUB_ENV key (which only emitted N-1's artifacts) in the smoke step.
  `sign-smoke-feed.sh` validated locally end-to-end with a throwaway key (production verifier accepts the
  signed smoke feed); the pwsh path is release-CI-validated (no local pwsh/Windows). actionlint +
  shellcheck clean.

All 8 folded. C4 is now merge-candidate pending the CI re-run on the fold (GATE 5) → GATE 6.
