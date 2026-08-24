# Independent Hardening Plan: SDK + GUI App

**Slug**: `independent-hardening` · **Branch**: `worktree-independent-hardening` · **Base**: main @ `9eff8dc`
**Mode**: fully solo ox-alpha · report-only · dynamic testing authorized (loopback) · runtime code only

## Objective

Hunt security vulnerabilities AND correctness bugs across:

- `packages/sdk` (~1.9k LOC TS): AcceleratorProver + transport
- `packages/accelerator` (~27k LOC Rust): core crate, Tauri app (`src-tauri`), headless server

Cross-platform: macOS / Linux / Windows.

## Independence contract

- Prior audit artifacts are OFF-LIMITS during execution: `audit/security/2026-08-21-mega-ready-*`,
  `audit/bugs/2026-08-21-mega-ready-*`, `implementations-plan/mega-ready-audit/**`.
- Zero external model consults (codex/fable/kimi) at any stage.
- Findings derive only from source reading + dynamic evidence gathered in THIS engagement.
- Findings numbered `IH-SEC-N` / `IH-BUG-N` — separate namespace from any prior numbering.

## Scope

IN: network ingress, origin/Host authorization, TLS+CA lifecycle, updater verification chain,
bb binary cache/downloader/spawn, Tauri IPC commands, autostart/uninstall/trust-store OS
integrations, config/state parsing, Windows ACLs, SDK probe/transport logic.
OUT: CI workflows, `infra/tofu`, landing/playground packages, release pipeline mechanics.

## Attack-surface clusters

| # | Cluster | Files | Core threat classes |
|---|---|---|---|
| C1 | SDK transport + prover | `packages/sdk/src/lib/{accelerator-transport,accelerator-prover}.ts` | probe→use TOCTOU port squatting (witness exfil), HTTPS→HTTP downgrade/TLS-strip, cert validation path, response parsing |
| C2 | Server ingress | `core/src/server.rs`, `core/src/server/{bind,host,auth,probe,prove}.rs` | loopback bind dual-stack bypass, Host-guard evasion, origin-tiered /health leak, body/header DoS |
| C3 | Authorization + verified sites | `core/src/authorization.rs`, `src-tauri/src/verified_sites.rs` | canonicalization adversary: punycode/confusables, trailing dots, case, port stripping, IPv6 literals, null-origin |
| C4 | PKI | `src-tauri/src/certs.rs`, `src-tauri/src/trust/*`, `src-tauri/src/server/tls.rs` | CA key storage perms, name-constraint enforcement, renewal races, NSS profile injection, partial-trust states |
| C5 | Updater chain | `src-tauri/src/updater.rs`, `update_marker.rs`, `core/src/{updater_state,update_manifest}.rs` | verify-before-write ordering, size-cap enforcement point, temp perms, marker manipulation, feed parsing |
| C6 | bb supply chain | `core/src/bb.rs`, `core/src/versions/*` | version-string path traversal in cache layout, downloader integrity, lease races, spawn arg injection, PATH hijack |
| C7 | IPC + state | `src-tauri/src/commands.rs`, `main.rs`, `tray.rs`, `crash_recovery.rs`, `core/src/config.rs` | every command as untrusted boundary, corrupted-state panics, serde bounds, popup rendering injection |
| C8 | OS edges | `src-tauri/src/{autostart,uninstall,windows}.rs`, `core/src/win_acl.rs`, `server/src/main.rs` | plist/registry/.desktop generation injection, healing TOCTOU, DACL ACE ordering, NSIS hook abuse, headless --allow-all |

## Phases

### Phase 0 — Home + recon doc ✅ gate: manifest registered, suites green baseline
Worktree homed; write this plan; write `recon.md` = exhaustive entry-point inventory
(every HTTP route, Tauri command, fs sink, process spawn, registry/keychain write).

### Phase 1 — Static cluster review (C1–C8)
Direct source tracing; untrusted-source→sink dataflow per cluster; candidate findings logged
in `findings.md` with file:line evidence + exploit hypothesis + confidence.
Gate: all 8 clusters covered; each ends "no finding" or evidence-backed candidates.

### Phase 2 — Dynamic red-team (live, macOS)
Build accelerator + headless server on isolated ports (run-isolation discipline:
ports from `~/.agents/ports.md`, owned process groups, real-disk datadir).
Attack matrices: Host variants (rebinding names, IDNA, `[::1]`, port confusion),
Origin variants (null/missing/case/punycode/trailing-dot/port-mismatch/scheme),
oversized + slow bodies, malformed JSON, concurrent port-squat race vs SDK probe→prove,
TLS wrong-name/expired-cert acceptance, HTTP-on-HTTPS-port confusion.
PoC scripts committed under audit dir.
Gate: every protocol-relevant Phase-1 candidate confirmed live or marked static-only w/ reason.

### Phase 3 — Cross-platform verification without those machines
`cargo check/clippy --target x86_64-pc-windows-gnu --lib --all-targets` (+ gitignored placeholder exe);
Linux-only modules by careful line-reasoning; Linux container only if Docker verified present.
Gate: zero new compile breaks; platform-divergence notes recorded per finding.

### Phase 4 — Triage + reports
Impact-bucketed Critical→Info; each finding: evidence lines, exploit scenario, PoC ref, fix direction.
Deliverables: `audit/security/<date>-independent-hardening/report.md`,
`audit/bugs/<date>-independent-hardening/report.md`.

### Phase 5 — Closeout
Lessons, index.md entry, eli5.html, single PR delivering reports + PoCs (no runtime code changes).
Pre-push validation: `bun run test` + `bun run lint` + `bun run lint:actions`.

## Known limitations

- Dynamic testing proves macOS behavior only; Linux/Windows findings stay source-level until CI/hardware confirms.
- No formal fuzzing infra — structured malformed-input scripts instead of coverage-guided fuzzing.
- Same-user local attackers in scope; cross-user boundaries only where code implies shared locations.
