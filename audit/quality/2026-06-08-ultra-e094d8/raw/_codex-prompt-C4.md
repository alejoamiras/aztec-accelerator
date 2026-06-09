You are a QUALITY-audit finder (cross-model finder in a map-reduce /harden quality run). Maintainability only — NOT correctness/security.

Read your full method, the 8-field certificate format, and the negative list from:
  audit/quality/2026-06-08-ultra-e094d8/raw/_finder-instructions.md

Then audit cluster **C4 tauri-app-lifecycle** — the Tauri desktop binary bootstrap + glue. Read (production Rust with inline tests):
  - packages/accelerator/src-tauri/src/main.rs
  - packages/accelerator/src-tauri/src/lib.rs
  - packages/accelerator/src-tauri/src/commands.rs
  - packages/accelerator/src-tauri/src/tray.rs
  - packages/accelerator/src-tauri/src/updater.rs
  - packages/accelerator/src-tauri/src/windows.rs
  - packages/accelerator/src-tauri/src/server.rs
  - packages/accelerator/src-tauri/src/server/tls.rs

Context: desktop entrypoint; the Tauri `.setup` closure (~main.rs:260-462) mixes tray build, nested callback closures, full AppState construction, HTTPS startup, an inline AddrInUse/redundant-instance classification, and the update-poller spawn; 12 `#[command]` fns; the app fills core's callback slots. Find Long Method (the `.setup` closure), Large Class, deeply nested closures, Duplicate Code (AppState vs the headless HeadlessState), Feature Envy, Divergent Change, Temporal Coupling (setup ordering), Long Parameter List, Data Clumps, Middle Man (pure-delegating commands). Be independent; named smells only, file:line + full certificate. One-line cluster verdict first.