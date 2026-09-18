# Lessons — independent-hardening

## Phase 3 (cross-platform)
- src-tauri's Windows-GNU check needs mingw-w64 (cc-rs dep) — not installed on this Mac; core
  crate checks fine without it. Don't install toolchains ad hoc; CI's windows lane covers it.
- The windows-target clippy run immediately surfaced IH-BUG-2 (duplicated cfg) — the repo's own
  CLAUDE.md advice ("check platform-gated code on the Windows target") pays for itself.

## Phase 2 (dynamic)
- Headless server is the ideal dynamic test bed: no GUI, deny-by-default, same ingress code as
  desktop. Build once (`cargo build -p accelerator-server --release`), attack freely.
- curl's `-w %{http_code}` + `-o` file pattern gives a compact attack-matrix table; `000` means
  client timeout — which is itself signal (the version-download stall observation, IH-BUG-3).
- Port squat PoC needs no SDK stack: shape-matched /health + capturing /prove proves the
  mechanism; full-SDK repro adds nothing.

## Phase 1 (static)
- This codebase annotates its own threat model in doc comments (SEC-01a/05, F-xxx refs). Reading
  comments AFTER deriving the invariant independently kept the review honest: every claim I
  spot-tested live held.
