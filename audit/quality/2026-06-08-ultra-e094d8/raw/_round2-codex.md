- **CrashRecovery finalize:** `packages/accelerator/src-tauri/src/crash_recovery.rs:16-43` is **Speculative Generality**, but **local/minor**, not architectural. I would **not** keep it as a standalone report finding. At most: one-line minor cleanup note.

- **Anti-anchoring on the 10 `VALID / CODEX-MISSED`: keep only 2 standalone.**
  - **REPORT-WORTHY**
  - `C1 — prove/resolve_version status ownership split`: hidden hot-path state machine across [core/src/server/prove.rs:64](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server/prove.rs:64), [94](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server/prove.rs:94), [160](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/core/src/server/prove.rs:160). Real local change-cost.
  - `C4 — duplicated HTTPS startup across main/commands`: real cross-entrypoint duplication at [src-tauri/src/main.rs:72-89](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:72) and [src-tauri/src/commands.rs:155-164](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/commands.rs:155). Worth keeping.

  - **FOLD**
  - `C2 — macOS Gatekeeper tail inside download_bb`: **FOLD** into Codex `C2 versions.rs hotspot`.
  - `C2 — platform identity smeared across cfg ladders`: **FOLD** into Codex `C2 versions.rs hotspot`.
  - `C3 — duplicated URL parse/host normalize`: **FOLD** into Codex `C3 Origin identity lives in raw strings` / approved-origin lifecycle split.
  - `C6 — #probeAndParseHealth long method`: **FOLD** into Codex `C6 transport split across two HTTP stacks` + `endpoint/protocol/cache temporal coupling`.

  - **DROP**
  - `C2 — duplicated xattr/codesign subprocess skeleton`: too small; same-file helper extraction, below final-report bar.
  - `C4 — auth popup label/close logic split`: tiny helper extraction; not enough standalone change-cost.
  - `C5 — duplicated security CLI wrapper scaffolding`: local boilerplate, secondary to the stronger cert-path clump.
  - `C5 — duplicated section/doc markers`: comment hygiene only.

- **Net:** from those 10, I would keep **2**, fold **4**, drop **4**.

- **Final gap check:** **no new gaps.**