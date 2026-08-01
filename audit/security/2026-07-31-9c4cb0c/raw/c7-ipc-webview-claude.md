# c7-ipc-webview — Claude (Opus) raw findings

## F-C7-1 — The auth-popup click-steal guard can be fully expired when a PROMOTED popup's buttons become clickable (consent bypass on a permanent origin grant)

**Impact.** Authorization (user consent) → Confidentiality: an `Allow` writes a **permanent**
`approved_origins` entry, after which that origin submits `/prove` jobs (private witnesses) forever with
no further prompt. Blast radius: one origin per stolen click, permanent and reusable from any page/iframe
on that origin. Vector network (any web page creates the popups); complexity low-to-moderate (attacker
controls popup count, origins, ordering; must win a re-click race); no privileges; user interaction
required (one legitimate click + one reflexive re-click).
**Confidence MODERATE** — the mechanism is certain from code; exploitation depends on human re-click
timing, so the vulnerability is high-confidence and the exploitation RATE uncertain.
**CWE-1021**, CWE-451, adjacent CWE-367. OWASP A01:2021.

**Trace.** Two web origins each POST `/prove` → `core/src/server/auth.rs:64` `auth_manager.request(&origin)`
→ first becomes ACTIVE, second is QUEUED (`authorization.rs:247-268`). Both popups are **built and
visible, same size, same place** (`windows.rs:197-208` 400×300, `:75` `.center()`); the queued one is built
unfocused (`:81` `.focused(false)`) so it sits directly UNDER the always-on-top active one at identical
coordinates → user resolves A (`authorize.js:107-108` → `commands.rs:174` `resolve_active` → `:180` closes
A → `:183` `arm_active_popup(next)`) → `commands.rs:226-233` `set_always_on_top(true)` + `set_focus()` on
B; B's webview fires `focus` → `bridge.js:34-37` `rearmInputGuard()` arms for **700 ms** (`:19,30,40-42`)
→ **THE GAP:** B's buttons are still `disabled`, enabled only by B's next poll — `authorize.js:81-87`
self-schedules at **1000 ms**, so the enable lands anywhere in `[0, ~1000] ms` after promotion (`:68`
`setControlsEnabled(info.active)`), and **nothing re-arms the guard when the buttons are enabled**
(`rearmInputGuard` is called only from focus/pageshow) → when poll delay > 700 ms (≈30% of promotions) B's
Allow becomes clickable with `isClickGuardActive() === false` (`bridge.js:108` passes through) →
`commands.rs:174` `resolve_active(Allow)` → `auth.rs:89-116` persists the origin → later requests
short-circuit at `auth.rs:44-55`.

**Missing control.** The guard is not re-armed (nor the click re-gated) at the `disabled → enabled`
transition. The two clocks that must align — "this control just became actionable" and "ignore input for
700 ms" — are driven by different events (native focus vs a 1 s IPC poll). A `rearmInputGuard()` on the
false→true edge of `info.active`, or gating on `enabledAt` rather than `focusedAt`, closes it.

**Exploit.** Attacker page loads a hidden iframe of a second origin; both fire `/prove` in the same tick →
popup A (active, on top) and popup B (queued, invisible beneath A, pixel-identical) → user expects the
prompt and clicks **Allow** on A → A closes, B is promoted into the EXACT same rectangle with Allow in the
same pixels; to the user the click looks like it didn't register → user re-clicks the same spot (typical
"nothing happened" latency ~0.5–1.5 s) → if B's poll enabled the buttons after the 700 ms elapsed, the
click is accepted and the second origin is permanently approved — an origin not visibly tied to the dapp,
surviving loss of the first domain.

**Why mitigations fail.** The 700 ms guard is armed from FOCUS, not from ENABLE: it fully protects the
FIRST popup (module eval happens after native focus, buttons enable within ms at `authorize.js:87`'s
`setTimeout(…,0)`), but for a PROMOTED popup the protective and actionable windows are misaligned. The
server-side arbiter (`authorization.rs:371`) enforces "only the active popup may decide" — it does not care
WHY the button was clicked. The button-disable while queued is precisely the lag that consumes the guard.
The distinct-origin display is correct and unspoofable but is rendered for only a fraction of a second
before the re-click. **Tests cannot catch it:** the Playwright mock sets `window.__CLICK_GUARD_MS__ = 0`
(`e2e/tauri-mock.js:14`) and the WebDriver spec sleeps past it
(`e2e-webdriver/auth-flow.spec.ts:142`), so no test exercises promotion-path guard timing.

**Worst instance (deterministic, no race).** If the user clicked the queued popup earlier — possible by
dragging the active popup aside, exposing the identically-placed queued ones — B ALREADY holds focus, so
`set_focus()` at `commands.rs:230` fires **no focus event at all**; B's `inputArmedAt` is minutes stale and
the promoted popup goes live with **no guard whatsoever**.

**Instances.** `bridge.js:30,34-37,40-42,104-108`; `authorize.js:68,81-87`; `commands.rs:226-233`
(`arm_active_popup`) and `:238-253` (`spawn_active_deny_timer` — same misalignment on the 60 s auto-deny
promotion path).

## F-C7-2 — Three of the four unsolicited auto-focused consent windows opt OUT of the click-steal guard, contradicting the invariant `bridge.js` asserts

**Impact.** Integrity/Authorization (consent for a trust-store write and an OS persistence entry) +
Availability (renewal and update both `restart()`). Blast radius host-wide: a local CA trusted by every
browser profile, an autostart entry, a permanent auto-update opt-in. Vector local/physical-presence;
complexity HIGH (no adversary controls when these windows appear); no privileges; user interaction
required — an **accidental**-consent hazard, not an attacker-timed one.
**Confidence** HIGH on the defect (code is unambiguous), **LOW** on adversarial exploitability — no
attacker-controlled trigger could be constructed. Reported because the codebase asserts the OPPOSITE
invariant and a prior audit item (C9/D9) established this threat as in-scope. **CWE-1021**, CWE-451.

**Trace.** Auto-shown, unsolicited, `focus_on_create: true`: onboarding (`main.rs:690-693` →
`windows.rs:122-146`, `:143`); renewal (`main.rs:700-718`, `:716` → `windows.rs:150-166`, `:163`); update
prompt (`main.rs:317-326`, 5 s after launch then every 12 h → `main.rs:254` → `windows.rs:245-265`, `:262`).
Buttons wired **without** `guard`: `onboarding.js:86` → `commands.rs:566-616` `complete_onboarding` →
`enable_https_inner` (CA generation + `install_ca_trust`, **silent on Linux via certutil**) +
`set_autostart_inner` + `auto_update = true`, with all three toggles PRE-CHECKED
(`frontend/onboarding.html:26,39,52`); `renewal.js:5-11` → `commands.rs:625-667` `renew_cert` → rotation +
OS trust dialog + **immediate `restart()`** (`:666`); `update-prompt.js:8-15` → `commands.rs:711-762` →
download+install+restart, with the auto-update checkbox PRE-CHECKED (`frontend/update-prompt.html:15`) so
one click also permanently delegates future silent installs.
**Contradicted invariant:** `bridge.js:38-39` states *"every remaining consequential control is a button
wired through `wireButton({guard:true})`"* — only `authorize.js:107-108` actually passes it.

**Missing control.** The `guard` flag is opt-in per call site (`bridge.js:101,108`) and is not applied to
any consent control other than Allow/Deny; nothing structurally forces a new auto-shown window to adopt it.

**Story.** The app autostarts at login; ~5 s in, the update prompt is created centered and focused while
the user is still clicking desktop icons or a browser tab beneath it — a click landing on "Update Now"
installs immediately, restarts, and silently flips `auto_update = true` for good. The renewal window does
the same at startup on macOS/Windows. **The onboarding wizard on Linux is the sharpest:** one stolen click
on "Start" installs the local CA into every user NSS DB with **no OS prompt at all**, plus an autostart
entry. (macOS Keychain and Windows Root-store dialogs are a second gate; Linux by design has none.)

**Instances.** `onboarding.js:86`; `renewal.js:5-11`, `:12-16`; `update-prompt.js:8-15`, `:17-20`; stale
invariant `bridge.js:38-39`.

## CHECKED AND CLEARED

- **IPC gate complete and consistent.** All 19 commands in `main.rs:569-589` appear in exactly one
  capability (`gen/schemas/capabilities.json` confirms the effective ACL) AND carry a handler-side
  `require_label`/`require_auth_window`. No command is capability-granted without a handler check, none is
  handler-checked but capability-orphaned, no capability grants `core:*`/plugin surfaces.
  `withGlobalTauri: false`, `app.windows: []` (no static unguarded window).
- **Auth-popup label binding.** `auth-{sha256_16(request_id)}` over a v4 UUID (`authorization.rs:346`) —
  128-bit, unguessable; Tauri resolves the caller label from the native IPC message. `respond_auth`
  (`commands.rs:159-160`) and `get_pending_auth` (`:210-211`) re-derive and compare on every call;
  `resolve_active` (`authorization.rs:371`) is the server-side arbiter, so a queued popup cannot decide
  even if its renderer is coerced. `get_verified_info` deliberately checks only "is AN auth window" and
  returns a public build-time-curated display name — no leak.
- **Origin rendering.** The popup renders `get_pending_auth`'s server-authoritative origin, never the query
  param (`authorize.js:63-64`); everything is `textContent` (no `innerHTML` outside `list.innerHTML = ""`).
  `canonicalize_origin` (`core/src/authorization.rs:21-70`) guarantees pure ASCII: special schemes are
  IDNA-punycoded (homographs surface as `xn--`), extension schemes grammar-validated, forbidden host code
  points make whitespace/newline/markup injection impossible. **The prior bidi + long-origin fixes HOLD on
  every render path**: `.origin-line` has `unicode-bidi: isolate`, its container `.popup-detail` has
  `word-break: break-all; max-width:100%` (a 253-char host wraps/scrolls rather than hiding the registrable
  domain), and the Settings list mirrors it (`settings.js:89-92`).
- **Navigation / window creation.** `is_local_asset_url` (`windows.rs:19-31`) is per-platform exact,
  rejects credentials and explicit ports, rejects `data:`/`file:`/`javascript:`/look-alike suffixes; the
  `http://tauri.localhost:80` case normalizes to the same origin (not a bypass). Every window is built
  through `open_or_focus_window`, which applies both `on_navigation` and `on_new_window(Deny)` — no second
  creation path. Even a successful same-origin navigation gains nothing: capabilities key on WINDOW LABEL.
- **Config as input.** Fail-open on malformed JSON (`config.rs:127-135`) produces only SAFER defaults
  (`https_enabled:false`, `approved_origins:[]`, `auto_approve_localhost:false`). Approved origins ARE
  re-validated on load (`de_approved_origins`, `:142-160`: re-canonicalize, drop-invalid, dedupe, no silent
  trailing-dot migration), never trusted as written. Writes atomic + 0600/owner-DACL into a 0700 dir. The
  headless server never reaches the config-write branch, so its `auto_approve_localhost: true` cannot leak
  into the desktop config.
- **Startup / argv / env.** `--remove-ca-trust` (`main.rs:486-507`) takes no attacker-controlled path
  (hardcoded `live_ca_cert_path()`) and fails safe. Beyond the documented `BB_BINARY_PATH`, production env
  reads here are `AZTEC_ACCEL_NO_UPDATE`/`AZTEC_ACCEL_FORCE_UPDATE_CHECK` (`main.rs:216-226`) and
  `RUST_LOG` — all same-user. The `webdriver` feature (which would open an unauthenticated WebDriver server
  on :4445) is **not** in `default = []`.
- **Cross-cluster note raised by this agent:** `trust/windows.rs:41` and `crash_recovery.rs:389` locate
  system binaries via `%SystemRoot%`/`%windir%` — outside this file set; **C6 owns it (F-C6-3)**.
