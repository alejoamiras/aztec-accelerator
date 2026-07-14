# C9 / F-014 — authorize-popup-safety — plan (light tier)

## Summary
The authorization popup (`packages/accelerator/src-tauri/frontend/authorize.html`) renders the requesting
origin and lets the user Allow/Deny + "Remember". Two safety issues:
- **Origin display can hide the security-relevant part.** `authorize.html:38` sets the raw origin as
  `textContent` in `.origin-line`, which has no wrap/ellipsis rules (`style.css:368`) — a long origin either
  wraps (and, with a fixed popup body, can clip or push the Allow/Deny buttons off-screen) or is otherwise
  not guaranteed to keep the registrable domain visible. A default CSS `text-overflow: ellipsis` would be
  WORSE — right-truncation hides the host tail, letting `https://trusted.com.<long-evil>.attacker.tld` read
  as `https://trusted.com…`.
- **"Remember this site" defaults to CHECKED** (`authorize.html:23`) — a hasty Allow permanently trusts the
  origin. F-014 wants it default-UNCHECKED (deliberate opt-in to persistence).

The origin is already canonicalized server-side (`url::Url` at ingress, per CLAUDE.md) so punycode arrives
canonical — the popup must NOT decode it to homoglyphs.

## Fix (F-014)
1. **Remember unchecked** — remove `checked` from the `#remember` input (deliberate opt-in).
2. **Origin always fully reachable + registrable domain preserved.** An ORIGIN is `scheme://host:port` (no
   path), so the trust-relevant part is the host TAIL (registrable domain) + scheme. Render with a JS
   **middle-ellipsis** that ALWAYS keeps the scheme (start) + the host tail incl. port (end) visible; set the
   FULL origin as the element `title` + an accessible label (screen-reader + hover). Keep the popup body
   **scrollable** (`overflow-y:auto`) with the Allow/Deny buttons in a fixed footer OUTSIDE the scroll area,
   so they are always reachable regardless of origin length. Keep punycode as-received (no decode).
3. Preserve the recognized-site badge behavior (it already renders the curated friendly name + ✓).

## Assumptions
### Facts (verified)
- `authorize.html:38` raw-origin `textContent`; `:23` Remember `checked`; `.origin-line` (`style.css:368`)
  is monospace-only (no wrap/ellipsis). The origin is server-canonicalized (`url::Url`); the popup only
  displays it. Allow/Deny in `.popup-buttons` (`:26-29`); respond_auth echoes `remember` (`:52-53`).
### Inferences (verify in impl)
- A tail-preserving middle-ellipsis closes the "disguise via truncation" concern WITHOUT a full bundled PSL
  (the whole host tail stays visible, so the eTLD+1 is never hidden). A bundled PSL to precisely
  bold/highlight the eTLD+1 is a possible enhancement, but ~200 KB in a popup is heavy for a light cluster —
  surface to the audit.
### Asks (defaults chosen)
- A1: tail-preserving middle-ellipsis + full-origin title/aria + scrollable body + reachable footer, no
  bundled PSL (the security property holds without it) — chosen; audit may push for PSL highlighting.
- A2: Remember default unchecked — chosen.

## Phases

### Phase 1 — origin display safety + Remember-unchecked (+ tests)
- `authorize.html`: remove `checked`; render the origin via a `middleEllipsis(origin, max)` that preserves
  scheme + host-tail, set `title` + `aria-label` to the full origin. `style.css`: `.popup-container` body
  scrollable, `.popup-buttons` fixed/reachable footer, `.origin-line` no destructive right-ellipsis.
- **Validation gate:** a `bun:test` unit for `middleEllipsis` (extract it to a tiny testable module or inline
  + test): short origin unchanged; a long origin keeps the scheme + the full host tail (registrable domain)
  + shows `…` in the middle; never drops the tail; full origin preserved for `title`. Manually/Playwright
  (mock UI test, `test:e2e:ui`) assert the popup renders + buttons reachable + Remember unchecked. Layers:
  unit (+ Playwright mock where wired). Src-tauri/GUI E2E ⇒ CI per the HARD RULE.

## Security & Adversarial Considerations
- **Threat:** a look-alike origin (long subdomain / trailing-dot / homoglyph) disguised so the user Allows a
  malicious site believing it's a trusted one. Closed by: never hiding the registrable domain (tail-preserving
  ellipsis), full origin reachable (title/aria + scroll), punycode kept canonical (no homoglyph decode),
  Remember default-unchecked (no accidental persistent trust).
- **Residual:** without a bundled PSL the popup can't visually BOLD the exact eTLD+1 (it keeps the whole tail
  visible instead). Homoglyph attacks are mitigated by the server's punycode canonicalization + not decoding
  it here. Documented.

## Seeds (draft)
- `/goal`: F-014 fixed — origin middle-ellipsis preserves scheme+registrable-domain tail, full origin in
  title/aria, scrollable body + reachable Allow/Deny, punycode canonical, Remember default-unchecked; gate
  green; post-impl codex xhigh folded; PR into security-hardening CI green.
- `/loop 15m`: drive C9 — authorize.html middle-ellipsis + title/aria + Remember-unchecked + style.css
  scroll/footer. Test middleEllipsis. Commit/push. Consult codex on PSL-vs-tail.
