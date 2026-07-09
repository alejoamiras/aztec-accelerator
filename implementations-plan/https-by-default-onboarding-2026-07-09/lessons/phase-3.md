# Phase 3 — trust abstraction + Linux NSS + ubuntu cert-trust CI

## What shipped
- **`src-tauri/src/trust/` module** (`mod.rs` API/DTOs/dispatch, `macos.rs`, `linux.rs`, `stub.rs`).
  - `TrustReport { stores: Vec<StoreStatus> }` + `any_installed()`. `AnchorRef(Option<String>)` = opaque old-anchor id (macOS SHA-1 / Linux nickname) captured pre-swap for D4 rotation cleanup.
  - Public API: `install_ca_trust`/`remove_ca_trust`/`trust_status`/`is_ca_trusted` + rotation hooks `current_anchor`/`trust_new_anchor`/`remove_anchor`. Per-OS `imp` selected by cfg.
  - **macOS**: moved verbatim-behavior from certs.rs; absolute `/usr/bin/security` (S2).
  - **Linux**: user NSS only, no root. Hardened `certutil` resolution (known absolute paths; a `which` result accepted only if absolute AND neither it nor its parent dir is group/world-writable — S2/codex). Hand-rolled `profiles.ini` parser (pure, unit-tested, no rust-ini). Store discovery = `~/.pki/nssdb` (create-if-absent) + Firefox profiles from native/snap/flatpak roots (only existing `cert9.db` profiles). Per-anchor nickname `aztec-accelerator-ca-<8hex sha256(DER)>`. Delete by **nickname** (D4 Linux). Honest per-store reporting + sandboxed-Chromium disclaimer (M-2) + certutil-missing hint.
  - **stub** (Windows for now): reports not-installed, errors on install — Phase 4 makes it real.
- **certs.rs refactor**: removed the inline macOS trust fns; `install_ca_trust`/`is_ca_trusted` delegate to `crate::trust`; `rotate()` uses the trust hooks (capture old → trust-new → swap → remove-old); `CertPaths::remove` un-cfg'd (all OSes discard failed staging now); new `live_ca_cert_path()`.
- **commands.rs**: `enable_https`/`disable_https` unified cross-platform (no more macOS-only + stub); enable succeeds iff trust in ≥1 store (R3). New `get_trust_status` + `remove_https_trust` (D5 "Remove certificate trust").
- **main.rs**: Linux launch gate = Ready when certs valid (trust decoupled, R3). New `--remove-ca-trust` CLI (uninstaller/scripted cleanup; runs + exits before GUI).
- **settings.html**: HTTPS row now visible on macOS + Linux (Windows still hidden). Playwright specs updated (Linux visible / Windows hidden).
- **tauri.conf.json**: `.deb depends: ["libnss3-tools"]`.
- **CI**: new `cert-trust` job (ubuntu) in accelerator.yml — `apt-get install libnss3-tools` then `cargo test --test trust_linux -- --ignored`; wired into `accelerator-status`.

## Local validation (env now has certutil + rust + tauri deps)
- ✅ `cargo build`, `cargo clippy --all-targets -D warnings` clean.
- ✅ `cargo test`: 24 lib (incl. 5 new trust::linux — profiles.ini parse ×3, nickname stable, writable-binary reject) + 7 main.
- ✅ **Real cert-trust integration test RAN LOCALLY** (certutil installed): `cargo test --test trust_linux -- --ignored` → generate CA+leaf → install into real NSS db in throwaway $HOME → is_ca_trusted true → **leaf chain-validates through the name-constrained anchor** (`certutil -V -u V` — closes I5/R9/M-3, proves NSS accepts + honors the anchor) → remove → untrusted.
- ✅ actionlint, biome, cargo fmt, `bun run test` aggregate.

## Notable
- `macos.rs` is `#[cfg(target_os="macos")]` → NOT compiled on ubuntu CI (clippy/test run on ubuntu). First real compile = the macOS cert-trust CI leg (Phase 4) + release builds. Reviewed against the `imp::` interface by hand. Pre-existing gap (old macOS trust code had it too).
- Windows enable_https returns the stub error until Phase 4 — expected; the row is hidden there so users can't hit it.
