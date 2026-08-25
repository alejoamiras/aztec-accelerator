# Codex audit — round 1 (session 01a03b08-e71e-71c3-8e7d-9305f91d884d, gpt-5.6-sol xhigh, verdict: reject)
# Round-1 findings were addressed in plan.md v2 — see Decision ledger. Final fresh-context pass appended below when run.

## 1. ADVERSARIAL / SECURITY

- **[High] Certificate consent becomes factually false.** The proposed generic sentence, “Your system will ask you to approve it once,” contradicts `frontend-src/onboarding.js`: Linux explicitly gets no separate prompt, while Windows has different behavior. Preserve platform-specific copy. Also disclose that OS certificate UI may show the frozen name “Aztec Accelerator Local CA”; otherwise a Presto-branded flow produces an alarming identity mismatch.

- **[High] Consent invariants are underspecified.** Authorization safety is not merely the two CSS anti-spoof assertions. Existing tests also enforce server-sourced origin binding, raw origin alongside the recognition badge, no truncation, permanence disclosure, keyboard focusability, and reachable Allow/Deny buttons at 400×300. The update prompt binds approval to the displayed version. These must be immutable acceptance criteria; “update tests with copy” could otherwise normalize weakened semantics.

- **[Medium] The supply-chain argument is too optimistic.** Seven-day package age is only a quarantine window. `@resvg/resvg-js` executes native code during development, while downloaded font binaries reach OS font parsers. Require exact versions, lockfile integrity, upstream URLs and hashes, per-family licences, and a deterministic extraction/subsetting recipe. One `OFL.txt` plus prose provenance is insufficient for three families.

- **[Medium] `--check` proves consistency, not provenance or portability.** Regenerating with the current generator and comparing bytes cannot detect coordinated generator/output tampering, and byte stability across resvg native builds is unproven. Keep semantic validation and run byte verification in a pinned environment; record hashes of source SVGs/fonts and tool versions.

- **[Medium] The proposed light-theme silk error label fails normal-text contrast:** `#F04E42` on `#FFFAF1` is about 3.44:1; white on silk is about 3.58:1. Consent/error surfaces need automated contrast checks and must use `#B02D23` where text-sized.

## 2. ASSUMPTION-ATTACK

### Facts

- **[High] Fact 4 is false.** Absence of snapshots does not mean only enumerated strings break. Existing tests assert popup/window reachability and layout. The custom slider also invalidates tests that assume `HTMLInputElement.value` plus native `input`/`change` behavior.

- **[High] The identity-test claim is overstated.** `tauri-identity.test.ts` pins productName, identifier, bundle metadata, Cargo naming, and selected workflow paths. It does not pin the updater endpoint, CA common name/NSS prefix, every autostart/task/systemd name, npm names, or domains. Passing it does not prove every hard exclusion. Add an explicit forbidden-file/value diff gate.

- **[Medium] Fact 5 is literally false.** `_e2e-crash-recovery-windows.yml` has an unrestricted branch `push` trigger, albeit only for two paths outside this plan. The accurate claim is that planned paths trigger no push CI.

### Inferences

- **[High] The terminal-state inference contradicts the SDK.** `denied` and `version-mismatch` are followed by `fallback`; `#fallbackToWasm()` then runs the local prover and emits `receive`. Halting the dial or treating these as terminal can show failure while a successful proof continues.

- **[Medium] “Light-only fallback” contradicts the promised light-and-dark scope.** Dark-mode propagation through each supported webview must be resolved before approval, not deferred as an optional preview degradation.

- **[Medium] The ≤400KB font assumption ignores glyph coverage.** Error messages and user paths may contain characters outside a marketing-copy subset. Specify weights, styles, Unicode ranges, and fallback testing.

### Asks

- **[High] Missing owner decision:** where must users be warned that Presto installs/updates an OS-visible product and certificate named Aztec Accelerator?

- **[Medium] Missing compatibility floor:** minimum WebKitGTK/WebView versions for `color-mix()`, variable fonts, and `prefers-color-scheme`.

- **[Medium] Missing cross-platform tray decision:** macOS-template black versus visible Windows/Linux assets.

## 3. IMPLEMENTATION-CRITIQUE

- **[High] The gates are not approval-ready.** `bun install --frozen-lockfile? (...)` is not a command. After changing `package.json`/`bun.lock`, desktop `cargo test` fails its `build.rs` freshness guard unless `bun run --cwd packages/accelerator frontend:build` runs first. Playground phases omit `build` and the existing production-smoke project even though branch CI is absent.

- **[Medium] Do not replace the native range input for a visual rebrand.** A hand-rolled `role="slider"` with arrow handling loses native pointer, touch, Home/End/PageUp/PageDown, and assistive-technology behavior unless substantially rebuilt and tested. Restyle the existing input.

- **[Medium] Do not hand-write ICNS/ICO containers.** The already-installed Tauri CLI 2.10.1 accepts SVG input and generates platform assets. Its official guidance also recommends the 32px ICO layer first, conflicting with the plan’s 256→16 order. Use `tauri icon` for the app ladder; keep custom rendering only for tray frames/OG assets. [Tauri icon guidance](https://v2.tauri.app/develop/icons/)

- **[Medium] Neither outline is ideal.** Outline A’s global asset-first phase creates the same half-rebranded state used to reject B and cross-writes web assets from the accelerator package. Use a hybrid: fonts/assets owned by each surface; app icons in the app phase; each OG image with its web surface. B is better specifically in deferring binary outputs until designs settle.

- **[Medium] Add `color-scheme: light dark`, visual/contrast checks, mobile-nav keyboard tests, and real reduced-motion phase semantics.** A construction-time preference snapshot and manual screenshots are insufficient.

## 4. RECON CHECK

- **[High]** Recon incorrectly calls fallback-related phases terminal; actual SDK control flow proves otherwise.

- **[Medium]** The plan misses `src-tauri/src/main.rs:728`’s initial `"Status: Idle"` and accompanying byte-identity comments when rewording `display_text()`.

- **[Medium]** It preserves the 26 `include_bytes!` couplings, but ignores recon’s warning that template recoloring is macOS-only; pure-black tray PNGs render literally on Windows/Linux.

- **[Medium]** Font placement correctly avoids the wiped `frontend/assets/`, but the asset phase still needs `frontend:build` because its lockfile change invalidates the bundle manifest.

- **[Medium]** The ~80-literal sweep should be a committed, scoped test. A raw search for `Inter` matches `IntersectionObserver`, while a broad Aztec-name sweep collides with many intentionally frozen strings. WebDriver title updates are identified correctly, but deferring the only real run leaves that lockstep unproved.

reject (with blocking findings: inaccurate certificate-consent copy, incorrect terminal-phase model, incomplete frozen-identity guard, and non-executable/insufficient validation gates)

---

## Final fresh-context pass (NEW session `01a03b18-d7d5-7350-9cbb-482a053becea`, gpt-5.6-sol xhigh)

### Round 1 (on plan v2) — verdict: reject
Blocking: Phase 5 WebDriver gate did not launch/supervise the app wdio.conf.ts expects running;
onThemeChanged fallback not executable under the frontend capability model. Non-blocking: `tauri
icon` emits `128x128@2x.png` not 256/512 (normalize/stage); resvg-wasm requires explicit
`initWasm` + font loading; `color-mix` conflicts with the macOS 10.15 `minimumSystemVersion`
(static fallbacks needed); contrast guard must parse real CSS tokens (light `#189E62` on paper ≈
3.31:1 — success text needs a `-text` twin); Win/Linux pure-black tray omitted from the ledger.
Also: frozen-identity fixture must enumerate every hard exclusion. → all adopted in v2.1.

### Round 2 (on plan v2.1, resumed) — verdict: reject
Confirmed most items resolved. Remaining: the launcher lacked display provisioning for this
headless host (no DISPLAY, no Xvfb — CI provisions Xvfb/DBus/stalonetray); `window.eval` right
after `build()` can hit the pre-navigation document (use `on_page_load(Finished)` + reload
reapply + ThemeChanged); contrast guard's three-source contract predates Phase 4's landing
conversion (phase it two-source → three-source); static color-mix fallbacks must be
theme-specific (dark literals in dark blocks). → all adopted in v2.2 (host state verified:
DISPLAY empty, Xvfb/stalonetray absent).

### Round 3 (on plan v2.2, resumed) — verdict: APPROVE
"All four round-2 findings are genuinely resolved… The earlier Codex blockers and Fable conditions
remain resolved. The remaining bridging-copy decision is the intentional owner Ask. I found no new
security, gate-ordering, reuse, or executability blocker."

**approve**