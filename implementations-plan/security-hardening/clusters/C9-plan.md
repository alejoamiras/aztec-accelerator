# C9 / F-014 — authorize-popup-safety — plan (light tier) — REVISED after codex audit (REJECT)

## Summary
The authorization popup (`frontend/authorize.html`) must let the user make a trust decision on the TRUE
requesting origin. Issues (codex-verified):
- **Origin display can hide / misrepresent the identity.** `authorize.html:38` shows the raw origin; the
  master plan wanted "preserve the registrable domain". My first draft (middle-ellipsis, no PSL) is WRONG —
  a fixed-suffix ellipsis cannot guarantee eTLD+1 visibility (fails for private suffixes e.g.
  `…trusted.github.io`, ports, IPv6, long punycode labels) and a bounded `max` can't preserve an unbounded
  tail. The CORRECT, lighter approach (codex): show the COMPLETE canonical origin as visible text with NO
  truncation — this satisfies "preserve the registrable domain" by never hiding any of the host.
- **Real clipping risk**: `body{overflow:hidden}` (`style.css:21-30`) + a fixed-height centered flex
  container (`:267-275`) + unrestricted content growth can clip a long origin or push Allow/Deny off — not
  the (already-inherited `word-break:break-all`) wrapping. Content must be scrollable with the buttons always
  reachable.
- **"Remember" defaults CHECKED** (`authorize.html:23`) — accidental persistent trust.
- **Display↔decision not bound**: the popup reads `origin` from the query param, but `respond_auth` resolves
  by `requestId` and treats `origin` as diagnostics (`commands.rs:142-152`). The DISPLAYED origin should be
  the server's authoritative canonical origin, not a param that could disagree with the resolved request.

## Fix (folding codex)
1. **Full-origin display, no truncation, no PSL.** Remove any `middleEllipsis`. Render the complete origin
   as visible, keyboard-focusable, **selectable** text in a bounded, visibly-scrollable region labelled
   "Requesting origin", with `dir="ltr"` + `unicode-bidi: isolate` (defend against bidi visual reordering;
   punycode stays as-received — never decode A-labels). `title` may supplement but is NOT the only disclosure.
2. **Layout**: a scroll-content wrapper holds the origin; the `.popup-buttons` footer sits OUTSIDE it so
   Allow/Deny are always reachable inside the 400×300 window. Re-enable selection on the origin (the doc
   disables it globally, `style.css:21-31`).
3. **Remember default UNCHECKED** (`authorize.html:23`); label the primary as "Allow once" and the checkbox
   "Always allow this site" (persistent approval never default/primary).
4. **Display↔decision binding**: add a backend `get_pending_auth(request_id) -> Option<String>` returning the
   server-held canonical origin; the popup DISPLAYS that (not the raw query param), and Allow is disabled if
   it is missing/mismatched. Keep `textContent` (no `innerHTML`).

## Deferred (documented follow-ups — real but beyond a light cluster; tracked in the ledger)
- **Server-authoritative display binding** (`get_pending_auth(request_id)` returning the server-held
  canonical origin, so the DISPLAYED origin can't disagree with the resolved request): DEFERRED. In the
  production flow the server builds the popup URL with the same canonical origin + request id together
  (`windows.rs:88-93`), so a realistic disagreement needs app-internal URL mutation or renderer compromise
  (codex GATE-3 ranked this the lowest residual risk of the three deferrals). Backend command + capability.
  [Supersedes the earlier draft that put this in Phase 1.]
- **Focus-swap / stacked prompts** (up to 10 pending origins, each new popup centered+always-on-top+focused
  → a controlled subdomain can steal a click to a newly-focused prompt): serialize/queue the auth UI. Larger
  windowing change.
- **Extension-scheme host validation** (`authorization.rs:52-57` only `to_ascii_lowercase()` — bidi/zero-width/
  combining chars not rejected): add backend ASCII + extension-ID validation. Backend canonicalization change.

## Assumptions
### Facts (codex-verified)
- HTTP(S)/WS(S) origins are `CanonicalOrigin`-canonicalized (`authorization.rs:21-50`; rejects userinfo/
  path/query/fragment/trailing-dot, tests `:467-483`) → ASCII punycode/IP literals reach the popup.
- `authorize.html:38` raw-origin textContent; `:23` Remember checked; `.popup-detail` gives `word-break:
  break-all` (`style.css:282-292`); the clip risk is `body overflow:hidden` + fixed-height container.
- Playwright mock (`accelerator.yml:310-328`) + WebDriver (`:338-355`) cover this popup; existing tests
  assert the OLD checked default (`e2e/authorize.spec.ts:38-59`, `e2e-webdriver/auth-flow.spec.ts:162-190`)
  and the "allow without remember" test UNCHECKS via a click (`:222-246`) — both must be updated for the flip.
### Asks (defaults chosen)
- A1: FULL canonical-origin display (no truncation/PSL) — chosen (codex; satisfies the master invariant).
- A2: Remember default unchecked + "Allow once"/"Always allow" labels — chosen.
- A3: bind display to the server origin via `get_pending_auth` — chosen (display integrity).
- A4: focus-swap + extension-scheme validation DEFERRED as tracked follow-ups (not silently dropped) — chosen.

## Phases

### Phase 1 — full-origin display + layout + Remember-unchecked + display binding (+ tests)
- `authorize.html`: origin from `get_pending_auth(requestId)` (fallback: verify query origin == server's,
  else disable Allow); scrollable focusable selectable origin region + `dir`/`unicode-bidi`; footer outside
  scroll; remove `checked`; "Allow once"/"Always allow" labels. `style.css`: scroll wrapper + reachable
  footer + re-enable origin selection. `commands.rs`: `get_pending_auth`.
- **Validation gate:** update `e2e/authorize.spec.ts` + `e2e-webdriver/auth-flow.spec.ts` for the unchecked
  default + the changed "allow-once" flow; add Playwright cases — full origin fully reachable (long/IPv6/
  punycode/private-suffix), default unchecked, Allow/Deny inside the 400×300 viewport + keyboard-reachable,
  recognized badge preserved, display == server origin. `bun run --cwd packages/accelerator test:e2e:ui`
  (mock) locally; WebDriver/Tauri-GUI ⇒ CI per the HARD RULE. `cargo test`/clippy/fmt for the
  `get_pending_auth` command. Layers: unit + e2e(mock) + lint.

## Security & Adversarial Considerations
- **Threat:** the user Allows a malicious look-alike origin (long subdomain, private-suffix tenant, IDN/
  punycode, IPv6) believing it's trusted; or grants accidental persistent trust; or a display/decision
  mismatch shows origin A while resolving request B. Closed by: full canonical-origin display (nothing
  hidden), server-authoritative display binding, punycode-canonical + bidi-isolation, Remember default-off.
- **Residual (documented):** focus-swap stacked-prompt click-steal + extension-scheme non-ASCII host — tracked
  follow-ups. Homoglyph mitigated by server canonicalization + not decoding punycode.

## Seeds (draft)
- `/goal`: F-014 fixed — full canonical origin shown scrollable/selectable/reachable (no truncation/PSL),
  bound to the server origin via get_pending_auth, punycode canonical + bidi-isolated, Remember default-
  unchecked + Allow-once labels; e2e updated + green; post-impl codex xhigh folded; PR into
  security-hardening CI green.
- `/loop 15m`: drive C9 — full-origin display + layout + get_pending_auth binding + Remember-unchecked +
  label + update the e2e tests. `test:e2e:ui` mock locally. Commit/push. Consult codex on the binding.
