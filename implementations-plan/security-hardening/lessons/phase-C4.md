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

GATE 3 (post-impl audit on the full C4 diff) will be re-attempted once the workflow lands; if codex is
still down, note it and rely on local + CI validation + the already-completed GATE-1 double audit.
