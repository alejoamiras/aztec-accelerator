# Phase 5 (popup) — implementation + post-impl review

Shipped as PR #421 (split from the full plan: the owner chose popup-first after the autostart bug
turned the rename into its own arc).

## Process failure, recorded first

The PR was opened STRAIGHT from implementation — the blueprint's post-impl half (`/code-review max
--fix` → codex audit → fix loop) was skipped until the owner called it out. The review then found two
defects already pushed:

1. **The test suite wrote the developer's real config.** Unconditional persistence means every test
   driving an Allow through `authorize_origin` writes `approved_origins`. The temp-path injection was
   applied to the NEW test only; `prove_triggers_popup_for_unknown_origin` wrote
   `https://unknown-site.com` into this machine's actual `~/.aztec-accelerator/config.json`. Codex
   had flagged exactly this at plan stage — the fix was implemented for one test instead of being
   made the default. Isolation is now the helper's default (a later commit also threads the TempDir
   guard so RAII cleans up instead of `keep()`-leaking).
2. **A pushed commit failed `clippy -D warnings`** (`TempDir::into_path` deprecated). `cargo test`
   passes either way; the commit was pushed after tests without re-running clippy.

Lesson, same shape both times: verifying one layer and assuming the rest. The gate exists as a list
for a reason — run all of it, every push.

## Codex post-impl audit (session 019fa904-…, /tmp/codex-26HCrAqd)

- **Loop 1** (full diff at ee5066b): **approve**, 0 blocking, 6 Lows. Explicit no-findings on the
  four highest-risk questions: piggyback fan-out cannot persist an unseen origin (same-canonical-
  origin only, pre-existing); click-steal guard intact on both buttons; the disk test genuinely
  fails on regression; #permanence reachable at the real 400x300 (scrollable content, pinned buttons).
- All six Lows folded in b43e5f3: pub(crate) config_path; isClickGuardActive unexported; TempDir
  guard threaded (10 call sites); Settings bidi regression spec (54 desktop-ui specs now); README
  duplicate Deny bullets merged; decision doc precise about the save-failure window.
- **Schema churn finding rejected with reason**: the gen/schemas deletions are stale regeneration
  from #375 (dismiss/open_onboarding removed, schemas never re-committed — every local cargo build
  on main dirties the tree). Kept; codex loop 2 endorsed keeping them.
- **Loop 2** (fixes verified): **approve — "No fix-induced defect found."** Including the two spots
  I'd have bet against: the regex-threaded 3-tuple (one test's double binding shadows LEGALLY, each
  request completes first) and the new spec's selector (matches the span settings.js actually
  generates, computed-style assertions exercise the real CSS).

Consult-log note: the first resume passed the WRONG session id (the plan-audit's) with the post-impl
dir; the skill's mismatch guard refused to run. That guard earned its existence.
