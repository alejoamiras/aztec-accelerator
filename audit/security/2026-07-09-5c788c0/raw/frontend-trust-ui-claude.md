# Frontend Trust UI — Security Audit Findings (Claude)

Cluster: `frontend-trust-ui` (authorize.html, settings.html, update-prompt.html, tauri-bridge.js,
serve.json, tauri.conf.json, capabilities/default.json)

Scope note: `canonicalize_origin` (packages/accelerator/core/src/authorization.rs:21-58) and
`authorize_origin` (packages/accelerator/core/src/server/auth.rs:15-111) were read as direct
dependencies to establish exactly what string reaches the popup — they are not themselves in the
cluster file list, so no findings are certified against them, only used to build the trace below.

Classic homograph/RTL/zero-width spoofing of the raw origin string was checked and **does not**
hold as a finding: `show_popup(&origin, &request_id)` (core/src/server/auth.rs:71) passes a
`CanonicalOrigin` — built exclusively via `canonicalize_origin()`, which parses the raw `Origin`
header through `url::Url` (idna/UTS-46 host processing). Non-ASCII/Unicode hosts are punycode
(`xn--`)-encoded before ever reaching `windows.rs`/`authorize.html`, and the pinned test
`canonical_origin_idn_punycode_no_homograph_collision` (core/src/authorization.rs:602) confirms a
homograph cannot collide with an ASCII-approved or ASCII-verified origin. `authorize.html` also
renders the origin via `textContent` (authorize.html:38), not `innerHTML`, so no markup injection
exists even before that. This closes the most obvious version of the cluster's ask.

Two concrete, narrower gaps remain:

---

## Finding 1: Long attacker-controlled hostnames overflow the fixed, non-resizable, non-scrolling authorize popup with no truncation indicator, letting the attacker hide the true (malicious) eTLD+1 off-screen

**1. Title**: Unbounded origin length + `overflow:hidden`/fixed-size popup = silent visual truncation of the authorization origin (CWE-451 UI misrepresentation)

**2. Impact factors**:
- Confidentiality: not implicated.
- Integrity: violated — the user's approval decision is bound to an origin string they cannot fully see.
- Authorization: violated — a malicious origin can obtain a persisted (`remember` is checked by default) grant to call `/prove` under a false belief about which origin was approved.
- Blast radius: one user per successful phishing interaction; scales to "many users" since any dApp author can pick a crafted hostname and this is a generic technique, not a one-off bug.
- Data sensitivity: the durable grant lets the attacker's origin submit future proving-witness requests without re-prompting.
- Attack vector: local/network — reachable by any HTTP client (browser or local script) that can set an `Origin` header and register a domain of the attacker's choosing; no browser-side origin spoofing is needed because the attacker controls their own real domain's label structure.
- Attack complexity: low (register one long DNS name; no timing/race needed).
- Privileges required: none.
- User interaction: required (the user must click "Allow" on the popup — this IS the interaction being manipulated).

**3. Evidence confidence**: moderate. The code trace (no length cap in `canonicalize_origin`, no `max-height`/`overflow-y`/truncation CSS, fixed non-resizable window, `body{overflow:hidden}`) is verified directly from source. The exact pixel-level clipping behavior was reasoned from the CSS rules rather than confirmed with a live screenshot.

**4. OWASP category + CWE**: CWE-451 (User Interface (UI) Misrepresentation of Critical Information). Closest OWASP Top 10 (2021) mapping: A04:2021 – Insecure Design (no standard web category fits a desktop-popup UI-truncation issue precisely).

**5. Trace** (source → sink, file:line at every step):
- `packages/accelerator/core/src/server/auth.rs:24-42` — `authorize_origin` reads the raw `Origin` HTTP header (attacker/local-process-controlled) and passes it to `CanonicalOrigin::parse`.
- `packages/accelerator/core/src/authorization.rs:21-58` (`canonicalize_origin`) — validates scheme/path/query/fragment/userinfo but applies **no length bound** to `url.host_str()` before formatting `scheme://host[:port]`. A syntactically valid ASCII host can be many hundreds/thousands of characters (WHATWG host parsing, which `url` 2.5.8 / `idna` 1.1.0 implement, does not enforce total-domain-length ≤253 for pure-ASCII inputs — that check is a DNS-resolution-time concern, not a URL-parse-time one, matching real browser address-bar behavior).
- `packages/accelerator/core/src/server/auth.rs:63-71` — on first request for an unapproved origin, `show_popup(&origin, &request_id)` is invoked with the full, uncapped canonical origin string.
- `packages/accelerator/src-tauri/src/windows.rs:89-93` — `show_auth_popup_window` builds `authorize.html?origin={urlencoding::encode(origin)}&requestId=...` — the entire attacker-chosen host is carried verbatim into the popup URL.
- `packages/accelerator/src-tauri/src/windows.rs:94-104` — the window is built via `WebviewWindowBuilder` with `inner_size(400.0, 300.0)` and `.resizable(false)` — the user has **no way to resize or scroll** to reveal clipped content.
- `packages/accelerator/src-tauri/frontend/authorize.html:33-38` — `origin = params.get("origin")`; `document.getElementById("origin").textContent = origin` — the full string is written into the DOM with no truncation/ellipsis logic.
- `packages/accelerator/src-tauri/frontend/style.css:21-28` (`body { overflow: hidden; ... }`), `style.css:267-275` (`.popup-container { height: calc(100vh - 40px); ... }`, no `overflow`), `style.css:282-292` (`.popup-detail { word-break: break-all; max-width: 100%; }`, no `max-height`/`overflow-y`) — long content wraps onto many lines with no cap, and anything exceeding the fixed viewport height is clipped by the propagated `body` `overflow: hidden` with **zero visual affordance** (no scrollbar, no "…", no resize handle) signalling to the user that anything was cut off.
- `packages/accelerator/src-tauri/frontend/authorize.html:22-25` — `<input type="checkbox" id="remember" checked />` — "Remember this site" defaults to checked, so a single misdirected "Allow" click persists the (actually malicious) origin into `approved_origins`, granting it unprompted future access.

**6. Missing control**: (a) no maximum-length enforcement on the origin/host in `canonicalize_origin`; (b) no truncation-with-ellipsis or "origin is unusually long, inspect carefully" UI treatment; (c) no scroll affordance / resizable fallback in the popup window so a legitimately long (if rare) origin can still be fully inspected; (d) "Remember" defaulting to checked removes the natural rate-limit of re-prompting on each request.

**7. Exploit/violation scenario**:
1. Attacker registers/controls a domain such as `nulo.sh.account-verify.` + (300+ filler characters designed to look like normal subdomain/path noise) + `.evil-actor.com`, and serves a page from `https://nulo.sh.account-verify.[padding].evil-actor.com`.
2. Victim, who trusts `nulo.sh` (a real verified-sites.json entry), visits the attacker's page (e.g. via a phishing link).
3. The page's JS calls the accelerator's `/prove` endpoint. The browser sets `Origin: https://nulo.sh.account-verify.[padding].evil-actor.com` (a real, unspoofed browser-set header reflecting the attacker's actual — but visually crafted — origin).
4. `canonicalize_origin` accepts it (valid ASCII host, no path/query/userinfo) and the popup opens with the full string.
5. Because the string is very long, `word-break: break-all` wraps it across many lines; the fixed, non-resizable 400×300 window with `overflow: hidden` clips everything past the visible ~260px content height. Only the attacker-front-loaded prefix (`nulo.sh.account-verify...`) is visible; `evil-actor.com` — the actual security-relevant eTLD+1 — is pushed out of view with no scrollbar or ellipsis to hint that more text exists.
6. The victim, seeing what looks like a `nulo.sh`-rooted origin and no verified-checkmark (verified-sites lookup fails since this exact host isn't curated — but many users don't require the checkmark to trust a domain that *starts* with a name they recognize), clicks "Allow" with "Remember this site" checked (default).
7. The attacker's actual origin (`https://nulo.sh.account-verify....evil-actor.com`) is now durably approved and can submit `/prove` witness requests indefinitely without further prompting.

**8. Preconditions**: attacker can register/control any domain long enough to overflow ~260px of monospace 13px text in a ~360px-wide box (well under DNS's 253-octet practical ceiling, so trivially satisfiable); victim must click through one popup.

**9. Why existing mitigations fail**: The Host-allowlist guard (SEC-01a) and deny-by-default authorization are orthogonal — they gate which server the request reaches and whether an unapproved origin needs a popup at all, not what the popup subsequently *displays*. `canonicalize_origin`'s homograph defense (punycode normalization) is real and closes the classic IDN-lookalike attack, but it does not defend against a long *literal ASCII* hostname, which is a different technique entirely (visual truncation, not character-confusability) and canonicalization does nothing to bound string length. The verified-sites badge does not help either — this attack deliberately targets an *unverified* look-alike-prefixed origin, which correctly shows no checkmark (per VERIFIED_SITES.md's own caveat that the raw origin is "always shown, so check it before clicking Allow" — but here it explicitly is not fully shown).

**10. Instances** (same root cause: no length cap + no truncation affordance in a fixed/non-scrolling popup):
- `packages/accelerator/core/src/authorization.rs:21-58` (`canonicalize_origin` — no host length bound)
- `packages/accelerator/src-tauri/src/windows.rs:89-104` (`show_auth_popup_window` — fixed 400×300, `resizable(false)`)
- `packages/accelerator/src-tauri/frontend/authorize.html:33-38` (raw write to `textContent`, no truncation logic)
- `packages/accelerator/src-tauri/frontend/style.css:21-28,267-292` (`overflow: hidden` + `word-break: break-all` with no `max-height`/`overflow-y`/ellipsis)
- `packages/accelerator/src-tauri/frontend/authorize.html:23` (`remember` checkbox defaults to `checked`, amplifying the consequence of one misdirected click)

---

## Finding 2: Curated `verified-sites.json` `displayName` has no ASCII/homoglyph validation, unlike `origin` — a compromised/deceived curation step can render a bidi- or zero-width-manipulated "verified" label

**1. Title**: Missing non-ASCII rejection on the verified-site `displayName` field (asymmetric with the origin field's explicit guard)

**2. Impact factors**:
- Confidentiality: not implicated.
- Integrity: violated, but narrowly — only the human-readable brand label shown next to the ✓ badge is affected. The raw canonical origin string (the actual authorization-relevant ground truth) is rendered separately and is unaffected by this gap, and VERIFIED_SITES.md explicitly tells users the badge is a recognition aid, not a safety verdict, and to check the raw origin. This materially limits real-world impact versus Finding 1.
- Authorization: not bypassed — the badge only renders for a canonical origin that already exactly matches a curated entry (homograph collision is independently prevented, per `canonical_origin_idn_punycode_no_homograph_collision`), so this cannot manufacture a false-positive match for an attacker's own origin.
- Blast radius: one/few users per malicious entry that slips into a release, until noticed and reverted; requires the supply-chain precondition below.
- Attack vector: local trust-plane/supply-chain (a PR to `verified-sites.json`), not directly network-reachable by a random web page.
- Attack complexity: high — requires getting a malicious entry merged past a single-curator review (`@alejoamiras`, enforced via CODEOWNERS per `packages/accelerator/VERIFIED_SITES.md`).
- Privileges required: high (write access to the repo, or the ability to deceive the sole curator during PR review — the classic "Trojan Source" bidi trick that fools human diff review, CVE-2021-42574-style).
- User interaction: none beyond the popup already being shown (no extra interaction needed once the entry ships).

**3. Evidence confidence**: high on the code gap itself (verified directly); moderate on real-world exploitability given the human-review precondition.

**4. OWASP category + CWE**: CWE-451 (UI Misrepresentation of Critical Information) / CWE-1007 (Insufficient Visual Distinction of Homoglyphs). Closest OWASP mapping: A08:2021 – Software and Data Integrity Failures (unverified curated data reaching a trust UI).

**5. Trace**:
- `packages/accelerator/src-tauri/src/verified_sites.rs:93-115` (`try_load`) — for each `origin` in an entry, raw non-ASCII is explicitly rejected (`if !origin.is_ascii() { return Err(...) }`, verified_sites.rs:106-108) before canonicalization. **No equivalent check exists for `entry.display_name`** — it is only validated for `is_empty()`/`len() > 64` (verified_sites.rs:100-102), with no ASCII/homoglyph/bidi-control-character restriction.
- `packages/accelerator/src-tauri/src/commands.rs:101-109` (`get_verified_info`) — returns `VerifiedSiteDto { display_name: s.display_name.clone() }` verbatim to the frontend.
- `packages/accelerator/src-tauri/frontend/authorize.html:42-48` — `recognized.querySelector(".recognized-name").textContent = info.display_name` — written via `textContent`, which prevents markup injection but does **not** prevent Unicode bidi-control (U+202E RLO / U+2066-2069 isolates) or zero-width (U+200B, U+FEFF) characters from altering the *visual* rendering of the string, since bidi reordering is a Unicode text-rendering-layer behavior independent of the DOM API used to insert the text.

**6. Missing control**: no `is_ascii()` (or stricter homoglyph/bidi-control) validation on `display_name` in `verified_sites.rs::try_load`, despite the adjacent `origins` loop demonstrating the maintainers are already aware of exactly this class of risk for the sibling field.

**7. Exploit/violation scenario**: A malicious or compromised contributor opens a PR adding/editing a `verified-sites.json` entry with a `displayName` containing bidi override characters, e.g. `"Nulo Wallet\u{202E}\u{202D}"`-style payloads designed to visually reorder or obscure the label (or to make the GitHub PR diff itself misleading to the reviewer, à la Trojan Source). If merged, every popup for that entry's origin(s) renders the manipulated label next to the green ✓, potentially causing brand confusion (impersonating a different recognized project's name) even though the actual origin field is untouched and correctly displayed.

**8. Preconditions**: attacker needs a merged PR to `verified-sites.json` — either a compromised curator account, or a bidi/zero-width payload subtle enough to pass the single human reviewer's visual inspection during the documented "launch the Accelerator, trigger the popup... confirm the ✓ renders" validation step (VERIFIED_SITES.md "Adding an entry" §2), which checks that a badge appears at all, not that the label text is free of hidden control characters.

**9. Why existing mitigations fail**: This is exactly the class of "compromised upstream" scenario the audit brief calls out as in-scope. The CODEOWNERS-gated single-curator review is a real control, but it is a human visual-inspection step — the same class of control that the industry-wide "Trojan Source" disclosure showed is unreliable against bidi-override/zero-width payloads specifically because they're designed to defeat visual review. The code-level `is_ascii()` guard that closes this exact gap for `origin` (verified_sites.rs:106-108) was, per its own comment, added deliberately ("Reject non-ASCII at the RAW level — url::Url::parse auto-punycodes Unicode hosts, so checking the canonical form alone is too late") — but the same discipline was not extended to `display_name`.

**10. Instances**:
- `packages/accelerator/src-tauri/src/verified_sites.rs:93-115` (validation loop — missing check)
- `packages/accelerator/src-tauri/src/commands.rs:101-109` (`get_verified_info` — passes through verbatim)
- `packages/accelerator/src-tauri/frontend/authorize.html:42-48` (render sink)

---

## Not flagged (examined, no concrete bypass found)

- **Absence of an app-level CSP** (`tauri.conf.json` has no `app.security.csp`) + `"withGlobalTauri": true`: real amplifier of blast radius *if* a markup-injection primitive existed in one of these three pages, but no such sink was found — every dynamic value (`origin`, `requestId`, `info.display_name`, `currentVersion`/`newVersion`, `origins` list in settings.html) is written via `textContent`/`createElement`, never `innerHTML`/`eval`/`insertAdjacentHTML`. Per the audit rules, a missing defense-in-depth control without a concrete injection vector is not certified as a finding.
- **`capabilities/default.json` granting `process:default`/`updater:default`/`autostart:default` to all windows** (no per-window `windows` scoping visible in the file, so the grant is not restricted to e.g. only the settings window): a genuine least-privilege gap on paper, but exploiting it requires attacker-controlled script execution inside one of these windows, which — absent the CSP/injection sink above — has no demonstrated path. No bypass shown; not certified.
- **serve.json (`{"cleanUrls": false}`)**: this is a static-file-server config (unrelated to the shipped Tauri asset pipeline); no security-relevant behavior difference found for this cluster's threat model.
