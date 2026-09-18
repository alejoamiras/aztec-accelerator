# Lessons — mega-ready-audit

## Phase 1–3 (2026-08-21)

- **Subagent fleet was unreliable this session**: 4 of 5 `general` Task calls returned
  `state: completed` with EMPTY output (one full report did land). After the second empty return
  I stopped delegating and did all cluster reads directly — which the owner explicitly wanted
  anyway ("I want YOU to check for findings"). Lesson: on empty subagent returns, fall back to
  direct reads immediately rather than re-rolling; the work is verifiable either way.
- **Re-verification beats re-audit for already-hardened code.** Every claimed fix from two prior
  audit runs held up at source level; the real deltas were two "open" findings that later work
  (#446) had silently closed, and stale tracking — not bad fixes. The ledger-as-claims approach
  found exactly that without re-litigating settled ground.
- **Check your own memory against defaults before filing findings**: the "Firefox on Windows
  doesn't trust OS roots" candidate died on `security.enterprise_roots.enabled` defaulting true
  since FF68; the "macOS update races KeepAlive" candidate died on `SuccessfulExit:false`
  semantics. Both would have been embarrassing Mediums.
- **`std::os::windows::fs::junction` does not exist in stable std** (misremembered); dir symlinks
  via `symlink_dir` + privilege-tolerant skip is the portable test pattern.
- **Windows-target validation from macOS**: `rustup target add x86_64-pc-windows-gnu` then
  `cargo check --target x86_64-pc-windows-gnu --lib --all-targets` in `core` type-checks
  cfg(windows) TEST code too (`--all-targets` is the part that matters; `--lib` alone skips it).
- **commitlint limits bit twice** (header ≤100, body lines ≤100) — use `-m -m -m` with short lines.

## Codex review rounds (post-audit, PRs #468/#469)

- Session `01a02515-9ef1-7950-a183-38681e5efac3` (gpt-5.6-sol @ xhigh), 3 rounds → both PRs approved.
- Codex caught a real framing error I shipped: #345's residual does NOT need the Ed25519 key — control of the signed URL's content (release-infra/CDN) suffices to force the pre-verify buffering DoS. updater.rs:344 said so in its own comment; I over-narrowed it in my re-assessment. Lesson: when re-assessing severity, quote the source comment verbatim instead of paraphrasing from memory.
- Codex's "inheritance not proved" was correct and subtle: verify_owner_only checks type/SID/mask but not ACE flags; equality-by-coincidence is not inheritance. Fixed with INHERITED_ACE + SE_DACL_PROTECTED + child-dir/grandchild chain.
- Real Windows-runner lesson (found by CI, not by either model): elevated tokens default-create files owned by **Administrators** — an owner assertion on a std-created child can never hold there. Ownership is a secure_create_* guarantee only.
- Process bug I hit twice: committing from a subdirectory cwd landed commits on the wrong branch of the stack. Rule: check `git branch --show-current` before every commit in multi-arc worktrees.
