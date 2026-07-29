# Lessons — updater-dispatch-gate, phase 1 (implementation)

## Plan audit (codex, xhigh, session `019fae8f-585b-7cf3-8840-ce2d46bb269a`)

Verdict: reject, 4 blocking — all folded into plan.md rev 2 (see "Audit fold" section there).
The big one: my in-file `event_name` split would have misrouted every release (called workflows
see the CALLER's `github` context; releases ARE workflow_dispatch). I had flagged the hole
pre-audit but proposed an input discriminator; codex's separate-file fix is structurally better —
the secretless path now cannot reference prod secrets even by bug, because no `secrets.*` exists
in its file.

## A vacuous parse check, caught in the act

Wanted a pwsh parse gate for the 460-line ps1 without a Windows roundtrip. First docker attempt:
`[System.Management.Automation.Language.Parser]::ParseFile($f, [ref]$null, [ref]$e)` — the call
ITSELF errored ("[ref] cannot be applied to a variable that does not exist", `$e` undeclared
under `-Command`), the error variable stayed null, `if ($e)` fell to the else branch, and it
printed "ps1 parses clean" **without having parsed anything**. Exactly the vacuous-test pattern
this whole arc hunts: the checker must PROVE it ran (pre-declare the ref vars; assert on the
errors array, not on falsy). Re-ran properly → genuinely clean.

## YAML block scalars vs heredocs

Embedding the NSIS sentinel as a `cat <<'NSI'` heredoc inside a `run: |` step: heredoc content
lines MUST be indented at or beyond the block-scalar base indent (YAML strips exactly the base;
anything shallower TERMINATES the scalar mid-script), and the terminator goes exactly at base so
bash sees it at column 0. First write had the content at column 2 → invalid YAML. actionlint is
the gate that catches this class locally; it passed only after re-indent.

## Fixture compatibility is a read of the OLD code, not the new

The hygiene fix (config seed `safari_support` → `https_enabled`) is read by the REAL v1.0.7
binary on the call path. Verified at the TAG, not HEAD: `accelerator-v1.0.7`'s config.rs has no
`deny_unknown_fields` (unknown `https_enabled` ignored) and `safari_support` defaults false —
seed safe for both ends. Grep-every-consumer now includes consumers that only exist in git
history when a test feeds real old binaries.

## Bootstrap gate (audit blocker 2)

`gh workflow run` needs the workflow on the DEFAULT branch — a feature branch can't dispatch its
own new workflow. Bootstrap: temporary `push: branches: [worktree-updater-dispatch-gate]`
trigger; every push validates the dispatch job end-to-end; the trigger is REMOVED in the final
pre-merge commit, then a post-merge dispatch burn-in re-proves the real trigger path.

## Round 1 (bootstrap run 30469821837): double-base64 pubkey

N−1 build died at updater-artifact signing: "failed to decode pubkey: Missing encoded key in
public key". `tauri signer generate`'s `.pub` FILE is ALREADY the base64 document that
tauri.conf.json's `pubkey` holds — my `base64 -w0 ./eph.key.pub` added a second layer.
Fix: `cat`, not re-encode. Verified locally by generating a key with the repo CLI and
diffing formats against the committed pubkey — 5-minute local repro vs a 15-minute CI leg.
(The PRIVATE key file is also single-line base64, which is why `cat` into GITHUB_ENV was
always safe on the existing path.)
