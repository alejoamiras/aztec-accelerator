# Recognized Sites Registry

`verified-sites.json` is a curated list of origins that the Aztec Accelerator
maintainers recognize. When one of these origins requests authorization, the
popup shows the friendly name + a verified ✓ next to the raw origin, instead
of just the raw origin string.

## **NOT a security guarantee**

The verified ✓ means **we (the maintainers) recognize this origin string**.
It does **not** mean the site is currently trustworthy.

- If an attacker hijacks DNS for a listed domain, the popup will still show
  the verified ✓.
- If a listed Chrome extension's auto-update is compromised, the popup will
  still show the verified ✓.
- If you reuse a verified name in a phishing context (typosquatted origin
  not on the list, etc.), the badge will NOT appear — but the raw URL is
  always shown, so check it before clicking Allow.

The badge is a **recognition aid**, not a safety verdict. It helps users
notice when an origin they personally trust is the one asking for permission,
especially when origin strings are opaque (e.g. `chrome-extension://...`).

## Format

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
    }
  ]
}
```

| Field | Required | Notes |
|-------|----------|-------|
| `schemaVersion` | yes | Currently `1`. Bump on breaking changes. |
| `entries[].origins[]` | yes | Canonical form: `scheme://host[:port]`, no path, no trailing slash. ASCII only — use Punycode A-labels for IDN (`xn--nlo-zna.sh`, NOT `nülo.sh`). |
| `entries[].displayName` | yes | Human-readable name shown in the popup. 1–64 chars. |
| `entries[].description` | no | Curator note. **NOT shown in the popup** — kept for the registry / future Settings UI. |
| `entries[].curatedBy` | yes | GitHub username of the curator who reviewed/added the entry. |
| `entries[].addedAt` | yes | ISO date — audit trail only. |

## Adding an entry

1. Open a PR adding the entry to `verified-sites.json`.
2. Use the canonical origin form. Validate locally: launch the Accelerator,
   trigger the popup from the listed origin, confirm the ✓ renders.
3. Review by the curator (currently `@alejoamiras` — enforced via
   `.github/CODEOWNERS`).
4. PR-gate runs `cargo test verified_sites::tests::embedded_registry_loads`
   against the real embedded JSON. Malformed entries fail the gate.

## Browser extensions

| Browser | Scheme | Curatable? |
|---------|--------|------------|
| Chrome | `chrome-extension://<id>` | **Yes** — IDs derive from the extension's manifest `key` (or the signing key) and are stable across installs. |
| Firefox | `moz-extension://<uuid>` | **No** — temporary installs use random UUIDs; permanent IDs differ per user. Not curatable as a universal origin. |
| Safari | `safari-web-extension://<id>` | **No** — same reason as Firefox. |

V1 of the registry includes Chrome extensions only.

## Removal

Open a PR removing the entry. Reasons to remove:

- Project abandoned or domain ownership transferred.
- Scope no longer fits (e.g. the project is no longer Aztec-related).
- Identity changed (e.g. a major rebrand makes the existing `displayName`
  misleading).

## Runtime fallback

If the embedded JSON ever fails to parse in a release build, the registry
loads as empty and a `tracing::error!` is logged. The popup falls back to
showing only the raw origin — no crash. Debug builds (`cargo tauri dev`)
panic immediately so developer errors surface loudly.

Build-time integrity is also enforced by `build.rs`, which parses
`verified-sites.json` as JSON during `cargo build` — any malformed JSON
fails the build before it can be shipped.
