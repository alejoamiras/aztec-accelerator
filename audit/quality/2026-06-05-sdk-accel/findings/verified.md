# Verified findings (Phase 3 reduce + Phase 4 verify) — quality / sdk+accelerator

Dedup by root-cause+location. `both` = Claude AND Codex converged independently (strongest signal). Anchor lines spot-verified against source.

| ID | Impact | Found by | Smell | Root cause |
|---|---|---|---|---|
| Q1 | architectural | both | Temporary Field / Data Clump | `AppState` conflates headless + GUI (7 `Option` fields, server.rs:34-42) |
| Q2 | architectural | both | Long Method / Large Class | `/prove` (server.rs:487-577) + `authorize_origin` (288-406) workflow sprawl |
| Q3 | architectural | both | Primitive Obsession | Aztec version as raw `&str` (from_version:38, sort_key:133, sigs across versions/bb/server) |
| Q4 | architectural | both | Divergent Change / Parallel Impls | crash_recovery platform semantics leak to callers; divergent disable signatures |
| Q5 | architectural | both | Long Method + Temporal Coupling | SDK `checkAcceleratorStatus` (202-319) + scattered phase emission |
| Q6 | architectural | codex | Temporal Coupling / Shotgun Surgery | update flow spread across main/updater/commands/windows |
| Q7 | architectural | both | Shotgun Surgery | Safari/HTTPS split between startup (main.rs) + settings commands |
| Q8 | structural | both | Primitive Obsession (stringly-typed protocol) | `ProveError=(StatusCode,String)`; 19 `json!` errors; magic header literals |
| Q9 | structural | both | Duplicate Code | config lock-mutate-save ×6 (commands.rs); respond_update_prompt SWALLOWS save err (219-220) vs `?` elsewhere |
| Q10 | structural | both | Primitive Obsession (UI state in strings) | tray animation `text.contains("Proving")` (main.rs:356) coupled to display copy |
| Q11 | structural | both | Long Method | `download_bb` (versions.rs:272-421) — guard/stream/digest/extract+codesign |
| Q12 | structural | both | Primitive Obsession | SDK `AcceleratorStatus` flat-optionals + string phase events |
| Q13 | structural | both | Repeated Switches | copy-bb.ts platform/arch matrix in 3 places (31-46,152-170,172-178) |
| Q14 | structural | claude | Duplicate Code | `is_auto_approved` (authorization.rs:126-147) re-parses vs `canonicalize_origin` |
| Q15 | local | both | Duplicate Code | 60s auth-timeout literal: server.rs:361 + windows.rs:72 (no shared const) |

Minor bucket (local/cosmetic, single-model unless noted): SDK WASM-fallback dup (sdk:336/384); home-dir fallback inconsistency `"."`vs`"~"` (certs.rs:14, crash_recovery.rs:84 — both-ish); 3 window-open helpers (windows.rs:20/41/88); AppState clone stutter (main.rs:347/377/397); MAX_BODY_SIZE literal vs const (server.rs:213/485); versions_to_evict re-parses from_version (168-170); bb_asset_name clump (versions.rs:121/331); write_pem_file double 0o600 (certs.rs:177/193); certs test dup w/ stale consts 3650/825 (certs.rs:414/448); VerifiedSite 3 dead fields (verified_sites.rs:24-34); default_config_version serde indirection (config.rs:69); frontend popup scaffolding dup + global-bridge coupling (authorize/update-prompt.html).
