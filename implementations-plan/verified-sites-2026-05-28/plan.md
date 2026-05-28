# Verified Sites — friendly name + recognition marker in authorization popup

**Status**: APPROVED v2.2 (user chose Option A visual treatment; pending implementation after CI dedup completes)
**Date**: 2026-05-28
**Type**: Tier B (contained UI feature + curated data file)

## v2 changes from v1

Major revisions after dual audit (codex + opus subagent). Both audits converged strongly:

| What | Why | Source |
|------|-----|--------|
| **Promote `url::Url`-based canonicalization to Phase 1** (was Phase 2 TODO) | v1's `lowercase + trim slash` fails on default-port (`https://nulo.sh:443` vs `https://nulo.sh`), trailing dot, IDN/punycode, doubled slashes. Single normalizer must be used at popup lookup AND server ingress AND approved-origins persistence to prevent semantic drift. | codex + opus (must-fix) |
| **Apply canonicalization at server.rs:213 ingress, not just lookup** | `authorize_origin` currently takes raw Origin header → `AuthorizationManager::is_approved` does string-equality. If we canonicalize only for badge lookup, same-origin variants badge identically but persist as DIFFERENT approvals. Must unify. | codex + opus (must-fix) |
| **Mandatory `embedded_registry_loads()` test + `build.rs` validation** | `include_str!` is compile-time TEXT inclusion only. `serde_json::from_str` runs at startup. With `panic = "abort"` in release (verified at Cargo.toml:57), a malformed JSON is a launch-time brick. v1 promised a test but didn't put it in Phase 2 deliverables. | codex + opus (must-fix) |
| **Degrade panic-on-load → `log error + empty registry` in release builds** | Badge is non-critical UX. Bricking the app on a JSON error punishes users for our dev mistake. Keep `panic` in `#[cfg(debug_assertions)]`. | opus (should-fix) |
| **REWORD "Verified" → "Recognized by Aztec Accelerator maintainers"** | Both auditors agree: green ✓ + "Verified" borrows TLS/identity trust signals we've explicitly disclaimed. UI copy IS the real threat model — docs no one reads can't undo what a green check says in the moment. | codex + opus (should-fix) — **NEEDS USER DECISION** |
| **Drop `description` from popup** | "Web wallet and faucet for Aztec" in the popup voice reads as endorsement. Keep description in JSON for registry/docs only. | codex + opus (should-fix) |
| **Raw origin = equal-or-larger visual weight** | Don't demote the raw URL to a subtitle. Friendly name is a LABEL on the origin, not a REPLACEMENT for it. | opus (should-fix) |
| **Drop Firefox/Safari extension entries from v1 seed** | Firefox temporary installs use random UUIDs; permanent IDs depend on signing scheme + may differ per user. Codex confirms: per-browser policy, not a universal rule. Chrome IDs (manifest `key`-derived) are verifiable. | opus + codex (should-fix) |
| **Drop the hash-check idea from v1's security section** | A compromised CI can change both the JSON and its hash. Real protection = CODEOWNERS + Tauri release signing. | codex (should-fix) |
| **Add `.catch(() => {})` + reserve layout space** for the `invoke("get_verified_info")` call | Avoid popup flicker on slow IPC; harmless on error. | codex (nit) |
| **Promote CODEOWNERS for `verified-sites.json` to mandatory Phase 5 deliverable** (was open question) | Required to keep the curator's verdict the actual gate. | opus + codex |
| **Add `#[serde(rename_all = "camelCase")]`** to load structs | v1 example code omitted this; would panic at load with confusing serde error. | opus (nit) |

## Goal

When a known origin requests authorization, show a friendly name + a recognition marker + the raw origin in the popup. Unknown origins display exactly as today (raw origin only).

**Concrete examples**:
- Today: `https://nulo.sh` → user sees raw URL.
- After: `https://nulo.sh` → user sees **Nulo Wallet** + recognition marker + `https://nulo.sh` (same prominence).
- Today: `chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh` → user sees opaque ID.
- After: → user sees **Nulo Wallet Extension** + recognition marker + the extension ID (same prominence).

**Success criteria**:
1. `packages/accelerator/verified-sites.json` shipped inside the app bundle.
2. Authorization popup renders the friendly name + recognition marker for entries on the list; unknown entries unchanged.
3. `url::Url`-based canonicalization applied at server ingress, lookup, AND approved-origins persistence — one identity logic.
4. Build-time validation in `build.rs` + runtime fallback (panic in dev, log+empty in release) for the embedded JSON.
5. UI copy makes clear this is recognition, not a security guarantee.

## Non-goals

- Logo/icon rendering per entry (v2).
- Remote-fetched list (rejected in clarifying round).
- Open submission flow (rejected).
- Wildcards (rejected for v1).
- "Unverified" warning UX (rejected).
- Refactoring `is_auto_approved` localhost logic.
- Verifying signature/identity at runtime.

## Current state (verified by reading source)

### Authorization flow

- `packages/accelerator/src-tauri/src/authorization.rs`:
  - `AuthorizationManager::request(origin)` registers pending; one popup per origin.
  - `is_auto_approved(origin)` bypasses popup for localhost variants.
  - `is_approved(origin, approved_origins)` does **raw string equality** at `authorization.rs:96`.
  - DoS guard: `MAX_PENDING_ORIGINS = 10`.
- `packages/accelerator/src-tauri/src/server.rs:213`:
  - Takes `Origin` header as raw string → `is_approved(&origin, &cfg.approved_origins)`.
  - On approve, origin pushed raw into `cfg.approved_origins`. **No canonicalization at any step today.**
- `packages/accelerator/src-tauri/src/windows.rs:51`:
  - Popup launched via `WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))`.
  - URL: `authorize.html?origin={urlencoding::encode(origin)}`.
  - Label: `auth-{sha256_prefix_6_bytes(origin)}` (`commands.rs:105-114`).
- `packages/accelerator/src-tauri/frontend/authorize.html` (45 lines):
  - Reads `origin` from `window.location.search`.
  - Calls `invoke("respond_auth", { origin, allowed, remember })`.
- `packages/accelerator/src-tauri/frontend/tauri-bridge.js`:
  - Exposes `invoke = window.__TAURI__.core.invoke`.
- `packages/accelerator/src-tauri/Cargo.toml:57`: **`panic = "abort"` in `[profile.release]`** — confirmed.
- `packages/accelerator/src-tauri/capabilities/default.json`:
  - Permissions: `core:default`, `autostart:default`, `updater:default`, `process:default`.
  - No `windows:` constraint → grants apply to all webviews.
  - Custom commands registered via `invoke_handler!` are callable from any webview by default. New `get_verified_info` works without capability change.
- `packages/accelerator/src-tauri/src/main.rs:191-194`: state management via `.manage(config_state)` etc. and `invoke_handler!` registration of commands.

### Existing identity-string handling — divergence risk

`server.rs:213` ingest → `authorization.rs:96` compare → `config::save` persist: all use raw strings. Adding canonicalization for verified-sites lookup ONLY would create a divergence: same site shows the same badge but persists as different `approved_origins` entries depending on what the browser sent.

## Design v2

### Data format — `packages/accelerator/verified-sites.json`

```json
{
  "$schema": "./verified-sites.schema.json",
  "schemaVersion": 1,
  "entries": [
    {
      "origins": ["https://nulo.sh", "https://faucet.nulo.sh"],
      "displayName": "Nulo Wallet",
      "description": "Wallet and faucet for the Aztec network",
      "curatedBy": "alejoamiras",
      "addedAt": "2026-05-28"
    },
    {
      "origins": ["chrome-extension://bafbiogfmibdojbhphgpbmbfokmhbpeh"],
      "displayName": "Nulo Wallet (Chrome Extension)",
      "description": "Chrome extension for Nulo Wallet",
      "curatedBy": "alejoamiras",
      "addedAt": "2026-05-28"
    },
    {
      "origins": ["https://playground.aztec-accelerator.dev"],
      "displayName": "Aztec Accelerator Playground",
      "description": "Playground for testing the accelerator",
      "curatedBy": "alejoamiras",
      "addedAt": "2026-05-28"
    }
  ]
}
```

Note: dropped the "Official playground" wording per codex — neutral copy. Only Chrome extension included; Firefox/Safari deferred (see "Browser extension origin handling" below).

### Origin canonicalization — strict, single-source

**Add `url = "2"`** to `packages/accelerator/src-tauri/Cargo.toml`. Replace v1's string trim with:

```rust
/// Canonicalize an origin string per RFC 6454.
///
/// - For tuple-origin schemes (http/https/ws/wss): scheme + "://" + lowercased host + optional non-default port.
/// - For opaque-origin schemes (chrome-extension://, moz-extension://, safari-web-extension://):
///   scheme + "://" + lowercased opaque tail. Port/userinfo/password REJECTED for opaque too.
/// - Rejects inputs with path, query, fragment, or userinfo (universally).
/// - Rejects empty hosts (including hosts that normalize to empty after trailing-dot strip).
/// - Idempotent: canonicalize(canonicalize(x)) == canonicalize(x).
///
/// Returns None for unparseable / disallowed inputs (caller treats as "no match").
pub fn canonicalize_origin(input: &str) -> Option<String> {
    use url::Url;
    let url = Url::parse(input).ok()?;
    // Universal rejections.
    if !url.path().is_empty() && url.path() != "/" { return None; }
    if url.query().is_some() || url.fragment().is_some() { return None; }
    if !url.username().is_empty() || url.password().is_some() { return None; }
    match url.scheme() {
        "http" | "https" | "ws" | "wss" => {
            let host = url.host_str()?.to_ascii_lowercase();
            let host = host.trim_end_matches('.');
            if host.is_empty() { return None; }
            let port = url.port();  // None if default; explicit otherwise
            Some(match port {
                Some(p) => format!("{}://{}:{}", url.scheme(), host, p),
                None    => format!("{}://{}", url.scheme(), host),
            })
        }
        // Opaque schemes — EXACT scheme match, no prefix, no port allowed.
        scheme @ ("chrome-extension" | "moz-extension" | "safari-web-extension") => {
            if url.port().is_some() { return None; }
            // For opaque schemes, the ID is in the host position. Reject empty.
            let id = url.host_str()?.to_ascii_lowercase();
            if id.is_empty() { return None; }
            Some(format!("{scheme}://{id}"))
        }
        _ => None,
    }
}
```

(Exact code TBD in Phase 2 — sketch above; the implementation must handle every edge case below in unit tests.)

**Unit tests required (mandatory in Phase 2)**:
- `https://nulo.sh` == `https://nulo.sh:443` (default port elided)
- `HTTPS://NULO.SH` == `https://nulo.sh`
- `https://nulo.sh.` (trailing dot) == `https://nulo.sh`
- `https://nulo.sh/` (root path) == `https://nulo.sh`
- `https://nulo.sh//` reject (path content)
- `https://nulo.sh/admin` reject (path)
- `https://nulo.sh?x=1` reject (query)
- `https://nulo.sh#frag` reject (fragment)
- `https://user@nulo.sh` reject (username)
- `https://user:pass@nulo.sh` reject (userinfo + password)
- `https://` reject (empty host)
- `https://.` reject (host normalizes to empty after trim)
- `https://xn--nlo-zna.sh` (punycode) ≠ `https://nulo.sh` — distinct origins per browser. List curator must enter punycode form, not Unicode. Reject non-ASCII at the **raw-input** level before canonicalization (`url::Url::parse` auto-punycodes Unicode hosts — so checking the canonical form alone is insufficient).
- `chrome-extension://BAFBI...` == `chrome-extension://bafbi...` (lowercased ID)
- `chrome-extension://bafbi.../` == `chrome-extension://bafbi...` (trailing slash trimmed)
- `chrome-extension://bafbi...:1234` reject (port not allowed for opaque)
- `chrome-extension-malicious://bafbi...` reject (exact scheme match, not prefix)

### Apply canonicalization at ingress + lookup + persistence

In `server.rs:213-217`:

```rust
let origin = match headers.get(http::header::ORIGIN).and_then(|v| v.to_str().ok()) {
    Some(raw) => match canonicalize_origin(raw) {
        Some(canon) => canon,
        None => return Err(/* invalid_origin / 400 */),
    },
    None => return Ok(()),
};
```

In `authorization.rs:96` `is_approved`:

```rust
pub fn is_approved(origin: &str, approved_origins: &[String]) -> bool {
    let canon = match canonicalize_origin(origin) {
        Some(c) => c,
        None => return false,
    };
    Self::is_auto_approved(&canon) || approved_origins.iter().any(|o| o == &canon)
}
```

In `commands.rs respond_auth`:

```rust
let canon = canonicalize_origin(&origin).ok_or("invalid origin")?;
// persist canon, not raw origin
```

This is the v1-must-fix that both auditors raised. **Single identity logic** across the request path.

### Rust side — `verified_sites.rs`

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSite {
    pub display_name: String,
    pub description: Option<String>,
    #[allow(dead_code)] pub curated_by: String,
    #[allow(dead_code)] pub added_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedSitesFile {
    schema_version: u32,
    entries: Vec<VerifiedSitesEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedSitesEntry {
    origins: Vec<String>,
    display_name: String,
    description: Option<String>,
    curated_by: String,
    added_at: String,
}

const VERIFIED_SITES_JSON: &str = include_str!("../../verified-sites.json");
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default)]
pub struct VerifiedSitesRegistry {
    by_origin: HashMap<String, VerifiedSite>,
}

impl VerifiedSitesRegistry {
    /// Load the embedded registry. Returns an empty registry on parse/validation failure;
    /// in DEBUG builds, panics so dev errors are loud. In RELEASE, logs the error and continues —
    /// the badge is non-critical UX and shouldn't brick the binary.
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(r) => r,
            Err(e) => {
                #[cfg(debug_assertions)]
                panic!("verified-sites.json failed to load: {e}");
                #[cfg(not(debug_assertions))]
                {
                    tracing::error!("verified-sites.json failed to load — recognition badges disabled: {e}");
                    Self::default()
                }
            }
        }
    }

    fn try_load() -> Result<Self, String> {
        let file: VerifiedSitesFile = serde_json::from_str(VERIFIED_SITES_JSON)
            .map_err(|e| format!("parse: {e}"))?;
        if file.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!("schemaVersion {} != {CURRENT_SCHEMA_VERSION}", file.schema_version));
        }
        let mut by_origin = HashMap::new();
        for entry in file.entries {
            let site = VerifiedSite {
                display_name: entry.display_name,
                description: entry.description,
                curated_by: entry.curated_by,
                added_at: entry.added_at,
            };
            for origin in entry.origins {
                // Reject non-ASCII at the RAW input level — `url::Url::parse` auto-punycodes Unicode
                // hosts, so checking the canonical form is too late.
                if !origin.is_ascii() {
                    return Err(format!("non-ASCII origin in list (use punycode A-label): {origin}"));
                }
                let canon = crate::authorization::canonicalize_origin(&origin)
                    .ok_or_else(|| format!("invalid origin in list: {origin}"))?;
                if by_origin.insert(canon.clone(), site.clone()).is_some() {
                    return Err(format!("duplicate origin: {canon}"));
                }
            }
        }
        Ok(Self { by_origin })
    }

    pub fn lookup(&self, origin: &str) -> Option<&VerifiedSite> {
        let canon = crate::authorization::canonicalize_origin(origin)?;
        self.by_origin.get(&canon)
    }
}
```

### Build-time validation — `build.rs`

Either add to `packages/accelerator/src-tauri/build.rs` (where Tauri's build script already lives) a parse-check:

```rust
fn main() {
    tauri_build::build();

    // Fail the build if verified-sites.json is malformed.
    let json = include_str!("../verified-sites.json");
    serde_json::from_str::<serde_json::Value>(json)
        .expect("verified-sites.json must be valid JSON");
    println!("cargo:rerun-if-changed=../verified-sites.json");
}
```

This catches syntax errors at compile time. The runtime `try_load()` catches schema/origin-validation errors that the syntax check misses. **Belt + suspenders.**

Plus add a CI step running `cargo test verified_sites::tests::embedded_registry_loads` as a separate PR-gate check (Phase 4).

### Tauri command + registration

`commands.rs`:

```rust
pub type VerifiedSitesState = Arc<verified_sites::VerifiedSitesRegistry>;

#[derive(serde::Serialize)]
pub struct VerifiedSiteDto {
    pub display_name: String,
    // description NOT exposed — see "DTO discipline" below.
}

#[tauri::command]
pub fn get_verified_info(
    origin: String,
    state: tauri::State<'_, VerifiedSitesState>,
) -> Option<VerifiedSiteDto> {
    state.lookup(&origin).map(|s| VerifiedSiteDto {
        display_name: s.display_name.clone(),
    })
}
```

**DTO discipline**: `description` is intentionally NOT in the DTO struct (not just not-rendered). Returning unused fields creates dead IPC surface and makes it easy to accidentally reintroduce endorsement copy later. If a future Settings UI needs descriptions, add a separate DTO at that time.

`main.rs:190-194` add:

```rust
.manage::<commands::VerifiedSitesState>(Arc::new(verified_sites::VerifiedSitesRegistry::load()))
.invoke_handler(tauri::generate_handler![
    // ... existing
    commands::get_verified_info,
])
```

### Popup UI — `authorize.html`

```html
<div class="popup-container">
  <h2>A site wants to use the Aztec Accelerator</h2>

  <div class="popup-detail">
    <!-- Layout space reserved so badge appears without flicker -->
    <div id="recognized" class="recognized-row" hidden>
      <span class="recognized-name"></span>
      <span class="recognized-marker" aria-label="Recognized by Aztec Accelerator maintainers" title="Recognized by Aztec Accelerator maintainers">●</span>
    </div>
    <!-- Raw origin: always visible, EQUAL prominence to name (not subtitle) -->
    <div id="origin" class="origin-line"></div>
    <!-- Disclaimer: only shown for recognized entries -->
    <div id="recognized-hint" class="recognized-hint" hidden>
      Recognized by Aztec Accelerator maintainers. This is not a guarantee of safety.
    </div>
  </div>

  <label class="popup-remember">
    <input type="checkbox" id="remember" checked />
    Remember this site
  </label>
  <div class="popup-buttons">
    <button class="btn btn-secondary" id="deny">Deny</button>
    <button class="btn btn-primary" id="allow">Allow</button>
  </div>
</div>

<script>
  const params = new URLSearchParams(window.location.search);
  const origin = params.get("origin") || "unknown";
  document.getElementById("origin").textContent = origin;

  // Fetch verified info; render label if matched. Safe on error/timeout.
  invoke("get_verified_info", { origin })
    .then((info) => {
      if (!info) return;
      const recognized = document.getElementById("recognized");
      recognized.querySelector(".recognized-name").textContent = info.display_name;
      recognized.hidden = false;
      document.getElementById("recognized-hint").hidden = false;
    })
    .catch(() => { /* network/IPC error → render as unverified */ });

  function respond(allowed) {
    const remember = document.getElementById("remember").checked;
    return invoke("respond_auth", { origin, allowed, remember });
  }

  wireButton("allow", { disableAlso: "deny", onClick: () => respond(true) });
  wireButton("deny", { disableAlso: "allow", onClick: () => respond(false) });
</script>
```

### CSS — `style.css`

- `.recognized-row` — flex row, name + neutral marker. Same weight/size as `#origin`.
- `.recognized-name` — bold for readability, not "look at me".
- `.recognized-marker` — neutral color (slate/grey), not green. A bullet/star (`●` or SVG star icon), not a check.
- `.origin-line` — same font size as `.recognized-name`, slightly muted color. Always visible.
- `.recognized-hint` — small text below, italic. Only shown when recognized.

### Visual treatment — DECIDED: Option A (Twitter-style)

The user picked **Option A** at the approval gate: green ✓ + "Verified" label, the original Twitter/social-media recognition pattern.

Both audits recommended against this on threat-model grounds (green check borrows TLS/identity trust signals; auditor verdict: "A is wrong"). User overrode this with explicit awareness of the trade-off (they raised the DNS-hijack scenario themselves in the initial brief). Documented here for traceability, not to relitigate.

**Implementation requirements with Option A**:
- Green ✓ icon (slate-600 fallback color: `#16a34a` Tailwind green-600).
- "Verified" label or aria-only marker (aria-label="Verified by Aztec Accelerator maintainers").
- Raw origin visible at the same prominence as the name — kept from C (both auditors agreed; codex specifically said "Keeping raw origin at equal-or-higher visual weight is still the right call").
- The "Not a guarantee of safety" hint is NOT shown inline (preserves Twitter-style cleanness). Equivalent disclaimer lives only in `VERIFIED_SITES.md`.

CSS:
```css
.recognized-name { font-weight: 600; }
.verified-check {
  display: inline-block; width: 18px; height: 18px;
  vertical-align: -3px; margin-left: 6px;
  fill: #16a34a;
}
.origin-line { font-size: inherit; color: var(--muted, #6b7280); margin-top: 2px; }
```

HTML (updated):
```html
<div id="recognized" class="recognized-row" hidden>
  <span class="recognized-name"></span>
  <svg class="verified-check" aria-label="Verified by Aztec Accelerator maintainers" role="img" viewBox="0 0 24 24">
    <path d="M9 16.17 4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z"/>
  </svg>
</div>
<div id="origin" class="origin-line"></div>
```

### Browser extension origin handling

- **Chrome** (`chrome-extension://<id>`): IDs are author-key-derived (32-char a–p hex). Reproducible. Verifiable. ✅ Seed includes Nulo Chrome extension.
- **Firefox** (`moz-extension://<uuid>`): Each install gets a **random UUID** (https://extensionworkshop.com/documentation/develop/extensions-and-the-add-on-id/). You CAN set a stable add-on ID via signing but it's not the install UUID — the install UUID is what gets sent as the origin. So Firefox extension origins are NOT curatable in general. **Excluded from v1.**
- **Safari** (`safari-web-extension://<id>`): scheme varies; non-portable. **Excluded from v1.**
- Document explicitly in `VERIFIED_SITES.md`: Chrome only for now; Firefox / Safari treated as out-of-scope.

### Documentation — `packages/accelerator/VERIFIED_SITES.md`

```markdown
# Recognized Sites Registry

`verified-sites.json` is a curated list of origins the Aztec Accelerator team recognizes — they get a friendly name + a recognition marker in the authorization popup.

## NOT a security guarantee

A recognition marker means **we (the maintainers) recognize this origin string** — it does **not** mean the site is currently trustworthy. If an attacker hijacks DNS for a listed domain or compromises a listed extension's auto-update, the popup will still show the recognition marker.

The marker is a recognition aid: it helps users notice when an origin they trust is asking for permission, especially when the origin string is opaque (e.g. `chrome-extension://...`).

## Adding an entry

1. PR to `verified-sites.json`. Canonical origin form (`scheme://host[:port]`, no trailing slash, no path).
2. Use Punycode (A-label) for non-ASCII hosts (e.g. `xn--nlo-zna.sh`, not `nülo.sh`).
3. **Chrome extensions only** for `*-extension://` origins. Firefox/Safari install IDs are per-user random UUIDs and cannot be curated.
4. PR review by curator (`@alejoamiras`).
5. PR-gate auto-validates the JSON.

## Removal

PR removing the entry. Reasons: project abandoned, scope no longer fits, identity changed.
```

## Phases

### Phase 1 — Implement strict canonicalization

1. Add `url = "2"` to `packages/accelerator/src-tauri/Cargo.toml`.
2. Implement `canonicalize_origin` in `authorization.rs` per the design above.
3. Apply at `server.rs:213` ingress (invalid Origin header → `400 invalid_origin`), `is_approved` lookup, and `respond_auth` persistence.
4. **`approved_origins` migration policy**: on config load, canonicalize each entry. Drop-with-warn for entries that don't canonicalize (log count + values dropped, e.g. `tracing::warn!(dropped = ?dropped_list, "Dropped N un-canonicalizable approved_origins entries during migration")`). Dedupe survivors. Rewrite config file ONLY if the canonical set differs from the loaded set.
5. **Unit tests** for all canonicalization edge cases (see Design's mandatory list).
6. **Integration tests for the server-level change** (the new 400 behavior is a real semantic shift):
   - `Origin: https://nulo.sh/admin` → server returns 400 `invalid_origin` (was: silent miss).
   - `Origin: https://nulo.sh?x=1` → 400.
   - `Origin: https://user@nulo.sh` → 400.
   - `Origin: https://NULO.SH:443/` → canonicalized → matches an approved entry `https://nulo.sh`.
   - `Origin: chrome-extension://bafbi.../` (trailing slash) → matches `chrome-extension://bafbi...`.
7. **Migration smoke test**: load a config with a mix of canonicalizable + un-canonicalizable entries; assert dropped count is logged, surviving set is canonical and deduped, file is rewritten exactly when changed.

### Phase 2 — `verified-sites.json` + `verified_sites.rs`

1. Create `packages/accelerator/verified-sites.json` with seed entries (Nulo Wallet web, Nulo Chrome extension only, Aztec Playground).
2. Create `verified-sites.schema.json` (JSON Schema 2020-12) for editor autocomplete.
3. Implement `verified_sites.rs` with `try_load` returning `Result`, `load` with debug-panic / release-log-empty fallback.
4. **Mandatory tests** in `verified_sites.rs` — these MUST be in the normal PR-gate test run (`cargo test`), not an optional job:
   - `embedded_registry_loads()` — calls `VerifiedSitesRegistry::load()` against the actual embedded JSON. Asserts non-empty + seed-entry presence.
   - `try_load_rejects_invalid_origin` (origin with path)
   - `try_load_rejects_duplicate` (same canonical origin in two entries)
   - `try_load_rejects_non_ascii_host_raw` (Unicode at raw input level)
   - `try_load_rejects_schema_version_mismatch`
5. Add validation in `build.rs` (parse the JSON at compile time as a syntax check; `cargo:rerun-if-changed`).

### Phase 3 — Wire into authorization flow

1. Add `get_verified_info` Tauri command in `commands.rs` — returns only `display_name`, NOT `description`.
2. Register `VerifiedSitesRegistry::load()` in `main.rs` managed state.
3. Add to `invoke_handler!`.

### Phase 4 — UI

1. Update `authorize.html` per Design v2.
2. Add CSS to `style.css` (option C visual treatment by default; can pivot to A/B based on approval-gate decision).
3. Manual test: launch with verified origin → marker + name + raw origin all visible. Launch with unverified → only raw origin.

### Phase 5 — Tests + docs

1. Playwright snapshot tests: verified vs unverified rendering. Mock `invoke` to return seed data.
2. Check existing WebDriver e2e tests for the authorization popup. If they cover the auth flow, extend to assert the marker for a known origin + absent for unknown. If not, scope add separately.
3. Write `VERIFIED_SITES.md`.
4. Update `CLAUDE.md` Current State section.
5. **Add CODEOWNERS** for `packages/accelerator/verified-sites.json` + `packages/accelerator/src-tauri/src/verified_sites.rs`.

### Phase 6 — PR + release

1. Open PR. Run `bun run test` + `bun run lint:actions`.
2. Merge. Cut `1.0.2-rc.X` prerelease to validate.
3. Post-validation, cut `1.0.2` stable.

## Test strategy

| Layer | Tests | Phase |
|-------|-------|-------|
| Rust unit (canonicalization) | 10+ edge cases above | 1 |
| Rust unit (registry) | embedded_registry_loads + rejections | 2 |
| Build-time syntax check | `build.rs` JSON parse | 2 |
| Rust integration (auth path) | Server takes raw → canonicalizes → stores canonical → re-lookup matches | 1 |
| Playwright snapshot | Recognized vs unrecognized popup render | 5 |
| WebDriver E2E (if applicable) | Marker visible for `https://playground.aztec-accelerator.dev` | 5 |

## Security & Adversarial Considerations

**Headline**: This is a recognition aid, NOT a security boundary. Documented in `VERIFIED_SITES.md`, code comments, AND popup hint text. UI copy is part of the threat model.

### Threats considered

1. **DNS hijack / SSL stripping** of a listed domain. Accelerator sees `https://nulo.sh`. Marker shows. User clicks Allow. → **Out of scope** by design; user is aware (their own framing).
2. **Typosquatting** (`nul0.sh`, `nu1o.sh`, etc.) → no marker → identical to today's UX. **Mitigated by absence.**
3. **IDN homograph** (`nülo.sh` → `xn--nlo-zna.sh`) → load-time rejection of non-ASCII hosts in JSON forces curator to use punycode. Browser sends punycode. Lookup matches OR no marker. **Mitigated by load-time validation.**
4. **Default-port confusion** (`https://nulo.sh:443` vs `https://nulo.sh`) → canonical form elides default ports. **Mitigated by `url::Url::origin()`-derived normalization.**
5. **Trailing dot DNS** (`nulo.sh.`) → canonicalization strips trailing dot. **Mitigated.**
6. **Path/query injection** (`https://nulo.sh/?evil`) → `canonicalize_origin` rejects non-empty path/query/fragment/userinfo. Lookup returns None. **Mitigated.**
7. **Subdomain confusion** (`https://nulo.sh.attacker.com`) → distinct host, no entry, no marker. **Mitigated.**
8. **Extension ID hijack via Web Store compromise** → if Chrome Web Store pushes a malicious update with same ID, marker still shows. **Out of scope** — Web Store integrity is upstream.
9. **Browser extension impersonation across browsers** → Firefox/Safari excluded; can't be curated reliably. Documented.
10. **Embedded JSON tampering at build time** → mitigated by Tauri Ed25519 release signing + CODEOWNERS on the JSON file. Note: hash-check in repo is theater — would just be tampered alongside.
11. **Launch-time crash from malformed JSON** → mitigated by `try_load` fallback in release builds (log + empty registry).
12. **`get_verified_info` IPC exposure** → read-only command. Default capabilities already allow custom commands to be invoked from any webview. No new privilege.
13. **Popup flicker / IPC race** → resolved decision flow is independent of badge render; `.catch(() => {})` keeps popup functional on IPC error.

### What's intentionally NOT trying to be solved

- Live trustworthiness of a recognized origin (DNS, TLS, ownership change).
- Phishing detection beyond the absence-of-marker signal.
- Per-user customization of the list.

### Domain checklist

- **Frontend XSS**: `textContent` not `innerHTML`. ✅
- **Tauri capability scope**: confirmed `core:default` enables custom commands by default. ✅
- **Supply chain**: embedded + Tauri signing. ✅
- **Least privilege**: command is read-only against managed state. ✅

## Rollback

Additive feature. Two-level rollback:
- UI-only: short-circuit `invoke("get_verified_info")` in `authorize.html` → popup reverts to today's behavior. JSON still loaded but inert.
- Full revert: revert the PR.

The canonicalization changes in Phase 1 are NOT trivially revertible (they shift identity semantics). If those need reverting, also revert the migration step.

## Open questions

| Q | Resolution |
|---|-----------|
| `url::Url` opaque-origin behavior for `chrome-extension://`? | Need to verify in Phase 1 implementation. Sketch in Design assumes opaque-origin path; if `url::Url` differs, adapt accordingly. |
| Playwright `invoke` mock compatibility? | Existing Playwright mock tests for the popup likely mock `window.__TAURI__.core.invoke`. Verify in Phase 5. |
| Pre-existing `approved_origins` entries that aren't in canonical form? | Phase 1 step 4 includes a one-shot canonicalize-on-read on config load. Document in lessons. |
| Visual treatment (Option A / B / C)? | **DEFER TO USER AT APPROVAL GATE.** v2 codes option C (auditor-recommended). |
| Include Firefox/Safari extensions in seed? | **No** — design decision documented above. |

## Estimated scope

- `Cargo.toml`: +1 line (`url = "2"`)
- `authorization.rs` (canonicalization + apply): +60 lines / -10 lines
- `server.rs`: +5 lines (canonicalize at ingress, error on invalid)
- `commands.rs`: +20 lines
- `verified_sites.rs`: ~140 lines (incl. tests)
- `verified-sites.json`: ~30 lines
- `verified-sites.schema.json`: ~50 lines
- `build.rs`: +5 lines
- `authorize.html`: ~35 lines edited
- `style.css`: ~30 lines added
- `VERIFIED_SITES.md`: ~40 lines
- `CODEOWNERS`: ~3 lines added (or new file)
- Playwright snapshot: ~50 lines
- WebDriver test extension (if needed): ~20 lines
- Total: 1 PR, ~450 lines diff
- Implementation time: ~3 hours coding + 45 min tests + 30 min docs
