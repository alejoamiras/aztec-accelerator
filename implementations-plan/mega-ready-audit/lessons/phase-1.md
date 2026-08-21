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
