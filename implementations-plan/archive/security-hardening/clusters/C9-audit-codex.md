Reject. The plan’s central claim—“middle ellipsis preserves the registrable domain without a PSL”—is false and contradicts the master requirement at `implementations-plan/security-hardening/plan.md:54`.

### 1. Middle ellipsis / PSL

`C9-plan.md:20-25,34-40,57-61` overclaims what a fixed-width suffix can preserve. It works for a simple long left subdomain only when the retained character budget happens to contain the complete eTLD+1.

It fails for:

- Private or multi-label suffixes. For `tenant.github.io`, `github.io` is the suffix and the tenant is security-relevant. A long attacker tenant ending in `trusted` could display as `…trusted.github.io`.
- Long registrant or punycode labels: the cut can occur inside the registrable label.
- Ports, which consume the suffix budget and can push more hostname out.
- IPv6: it has no registrable domain, and character slicing can produce an ambiguous fragment such as `…:1]:5173`.
- Localhost, IP addresses, and extension origins, which require separate presentation rules.
- Any fixed `max`: it is mathematically impossible to preserve an unbounded “whole host tail” while also enforcing that maximum.

Trailing dots and userinfo are not popup cases: they are rejected at `packages/accelerator/core/src/authorization.rs:24-41`; the tests pin this at `:467-483,516-519`.

The lighter correct approach is to show the complete canonical origin as visible DOM text, wrapped in a bounded scroll area, with no truncation. If the design insists on a compact one-line summary that guarantees eTLD+1 visibility, then a maintained PSL is required and the eTLD+1 itself must never be ellipsized. Right-aligning or retaining the last N characters is not sufficient. For a desktop binary, ~200 KB is not a compelling reason to weaken a wallet authorization identity, but PSL can be avoided entirely by showing the full host.

Also, `C9-plan.md:23-25,47-48` says both “make `.popup-container` scrollable” and put the footer outside its scroll area. The buttons are currently children of that container at `authorize.html:26-29`; this requires a distinct scroll-content wrapper or an independently scrolling `.popup-detail`, not merely `overflow-y:auto` on the container.

### 2. Full-origin accessibility

`title` plus `aria-label` is inadequate as the primary disclosure:

- `title` is hover-dependent and unreliable for touch and keyboard users.
- `#origin` is a generic, non-focusable `<div>` at `authorize.html:20`; an accessible name on it is not consistently discoverable.
- The document disables selection globally at `style.css:21-31`.

Keep the full origin as actual visible text. Make its scroll region keyboard-focusable, selectable, visibly scrollable, and labelled “Requesting origin.” Add `dir="ltr"` plus `unicode-bidi: isolate`. A `title` can remain supplemental, but not the only full visual representation.

The existing origin already inherits `word-break: break-all` from `.popup-detail` at `style.css:282-292`; the plan’s claim that it has no wrapping is therefore inaccurate. The real clipping risk comes from `body { overflow:hidden }` at `style.css:21-30`, the fixed-height centered flex container at `:267-275`, and unrestricted content growth.

### 3. Punycode and visual controls

Keeping punycode is correct. Decoding A-labels would reintroduce Unicode homograph risk.

For HTTP(S)/WS(S), `CanonicalOrigin` parses and serializes the host before popup creation (`authorization.rs:21-50`), while rejecting userinfo, paths, queries, fragments, and trailing dots. These displayed hosts should therefore be ASCII punycode/IP literals.

That guarantee does not cover extension schemes. At `authorization.rs:52-57`, opaque extension hosts are merely passed through `to_ascii_lowercase()`. Unicode bidi controls, zero-width characters, or combining marks are not categorically rejected there. Add backend ASCII and scheme-specific extension-ID validation. The popup should still use LTR bidi isolation and visibly escape unexpected format/control characters as defense in depth.

`textContent` at `authorize.html:38,46` prevents XSS, but it does not prevent bidi visual reordering.

### 4. Remember and primary action

Removing `checked` at `authorize.html:23` is sufficient to make the existing primary button an ephemeral Allow: Allow already has `btn-primary` at `authorize.html:28` and `style.css:310-313`.

For clarity, label it “Allow once” and consider renaming the checkbox to “Always allow this site.” Persistent approval should never become the default or visually primary action.

### 5. Validation gate

The infrastructure is real, but the plan is incomplete:

- Mock Playwright exists at `packages/accelerator/package.json:10`, runs in CI at `.github/workflows/accelerator.yml:310-328`, and already covers this popup.
- Real Tauri WebDriver runs on Linux/macOS/Windows at `.github/workflows/accelerator.yml:338-355`.
- Existing tests explicitly assert the old checked default at `e2e/authorize.spec.ts:38-59` and `e2e-webdriver/auth-flow.spec.ts:162-190`; the latter persistence test must explicitly check Remember before Allow.
- The “allow without remembering” WebDriver test at `auth-flow.spec.ts:222-246` currently clicks the checkbox to uncheck it; after changing the default, that click would do the opposite.

A Bun unit is only real if it imports the production helper and is placed under the `scripts/` discovery path used by `package.json:12` and CI. “Inline + test” is not a concrete gate. Extracting a pure formatter is reasonable only if a formatter remains; with the safer full-origin design, remove `middleEllipsis` and test actual viewport behavior in Playwright: full origin reachable, default unchecked, actions inside the 400×300 viewport, keyboard reachability, IPv6/punycode/private-suffix cases, and recognized badge preservation.

### 6. Assumptions and missed adversarial angles

- Production origin provenance is sound: HTTP `Origin` → `CanonicalOrigin` at `server/auth.rs:24-41` → popup callback at `:63-72` → URL-encoded query at `windows.rs:88-93`.
- However, display and decision are not cryptographically bound in the popup. JavaScript reads both query parameters at `authorize.html:33-37`, while `respond_auth` resolves solely by `requestId` and treats `origin` as diagnostics at `commands.rs:142-152`. A modified query could therefore show one origin while resolving another request. Prefer `get_pending_auth(requestId)` returning the server-held canonical origin, or at minimum verify the supplied origin matches that request and disable Allow for missing/malformed parameters.
- Conventional web clickjacking is limited because this is a separate native, non-resizable window (`windows.rs:43-50`), not an iframe. The more realistic issue is focus swapping: every new popup is centered, always-on-top, and focused (`windows.rs:43-52,94-104`), while up to ten distinct origins may be pending (`authorization.rs:163-165`). Controlled subdomains can stack identical prompts so a click intended for one lands on a newly focused one. Serialize authorization UI globally or queue additional origins without opening/focusing another popup.
- Origin and recognized-name rendering use `textContent` (`authorize.html:38,46`), so retain that and do not introduce `innerHTML`.
- The plan’s no-PSL choice at `C9-plan.md:34-40` also directly contradicts both the master invariant at `security-hardening/plan.md:54` and the ledger description at `security-hardening/index.md:16`; that is not an implementation “default” the cluster may silently change.

VERDICT: reject (middle-ellipsis cannot guarantee registrable-domain visibility without PSL/full display; full-origin accessibility and concurrent popup focus-swap safety are unresolved)