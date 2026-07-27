# Platform Support

## Supported Platforms

| Platform | Architecture | Status | Notes |
|----------|-------------|--------|-------|
| **macOS 13+** (Ventura) | Apple Silicon (arm64) | Supported | Primary development platform |
| **macOS 13+** (Ventura) | Intel (x86_64) | Supported | Built and tested in CI |
| **Linux** | x86_64 | Supported | .deb and .AppImage provided |
| **Windows** | x86_64 | Supported | NSIS installer (per-user) |

## Encrypted Connection (HTTPS)

HTTPS between the browser and the accelerator is **default-on**, consented through the first-run
onboarding wizard, on **all three** desktop OSes (it was previously a macOS-only "Safari Support"
toggle). It gives an encrypted, authenticated loopback channel; Safari *requires* it (Safari blocks
plain HTTP from an HTTPS page), while Chrome/Firefox/Edge use it when the local certificate is trusted
and otherwise fall back to HTTP with no added latency.

The certificate is a **keyless local CA** (the CA signing key is generated in memory, signs one
`localhost` leaf, and is discarded — never written to disk, so the trusted anchor can mint nothing)
constrained to `127.0.0.1`, `::1`, and `localhost` via X.509 Name Constraints. The leaf is auto-renewed
within 30 days of expiry.

| OS | Trust store | Consent | Rotation re-trust | Uninstall |
|----|-------------|---------|-------------------|-----------|
| **macOS** | login Keychain (`security`) | password dialog on install | renewal consent window → password | Settings "Remove certificate trust"; or Keychain Access |
| **Windows** | CurrentUser `Root` (`certutil.exe`) | the wizard's *Start* click (no separate dialog is guaranteed) | renewal consent window | NSIS uninstaller removes it; or Settings "Remove certificate trust" |
| **Linux** | user NSS DBs — `~/.pki/nssdb` (Chrome/Chromium/Brave/Edge) + each Firefox profile — via `certutil` | the wizard's *Start* click (no OS dialog exists) | silent (user DBs need no auth) | Settings "Remove certificate trust"; or `aztec-accelerator --remove-ca-trust` |

**Linux notes.** Requires `certutil` (the `.deb` depends on `libnss3-tools`; the AppImage detects it and
degrades with an install hint if absent). Per-store trust status is shown honestly in the wizard/Settings.
**Sandboxed (snap/flatpak) Chromium keeps a private, confined trust store the app cannot reach** — it is
disclaimed, not silently claimed as covered. Firefox must be restarted to pick up a newly added anchor.

## macOS Details

- **Code-signed and notarized** via Apple Developer ID
- **Auto-update** via Ed25519-signed artifacts (tauri-plugin-updater)
- **Encrypted connection (HTTPS)** via the login Keychain — see [Encrypted Connection (HTTPS)](#encrypted-connection-https) above (this is what Safari requires)
- **System tray** app — no Dock icon by default (Accessory activation policy)
- **Start on Login** via LaunchAgent plist with crash recovery (KeepAlive + ThrottleInterval)

## Windows Details

- **NSIS installer** (per-user, `installMode: currentUser`)
- **Auto-update** via Ed25519-signed artifacts (tauri-plugin-updater)
- **Encrypted connection (HTTPS)** via the CurrentUser `Root` store — see [Encrypted Connection (HTTPS)](#encrypted-connection-https). The NSIS uninstaller removes the trust anchor on a real uninstall (guarded so it never fires during an auto-update)
- **Start on Login** via the autostart Run key; crash recovery via a Task Scheduler repeating trigger

## Linux Details

- **.deb** package for Debian/Ubuntu-based distros
- **.AppImage** for other distros (self-contained, no install needed)
- **System tray** requires a tray implementation (most desktop environments provide one; Wayland compositors may vary)
- **Crash recovery** via systemd user service with `Restart=on-failure`
- **Encrypted connection (HTTPS)** via user NSS databases (no root) — see [Encrypted Connection (HTTPS)](#encrypted-connection-https). Requires `certutil` (`.deb` depends on `libnss3-tools`)

### Wayland

The app uses GTK via WebKitGTK. Tray icon support depends on the compositor:
- GNOME: requires an extension (e.g., AppIndicator/KStatusNotifierItem)
- KDE Plasma: works out of the box
- Sway/wlroots: requires `waybar` or similar with tray support

## Browser Notes

### Chrome Local Network Access (Chrome 142+)

Chrome 142 (October 2025) began gating requests from public websites to loopback addresses behind a user permission prompt (Local Network Access); Chrome 145 splits it into `local-network` and `loopback-network` permissions. A dApp probing the accelerator from a public origin triggers the prompt on first use. If the user blocks it, the SDK sees the accelerator as offline and falls back to WASM proving — re-allowing requires resetting the permission in Chrome's site settings. The prompt is keyed on the destination address space, not the scheme: the accelerator's HTTPS mode does not exempt it.

Firefox and Safari have no equivalent gate as of mid-2026. Requests from `localhost`-served pages (local dev) are same-address-space and do not trigger the prompt.

## Security Model

### Localhost Authorization

The accelerator runs an HTTP server on `127.0.0.1:59833` (localhost only — not exposed to the network).

**Browser requests** (cross-origin): The `Origin` header is checked against the approved origins list. Unknown origins trigger a MetaMask-style authorization popup. Approved origins are persisted in `~/.aztec-accelerator/config.json`.

**Non-browser requests** (curl, scripts): No `Origin` header is sent, so requests are auto-approved. This is by design — `Origin` is a browser-only mechanism. The binding to `127.0.0.1` is the security boundary for non-browser access.

**Localhost origins** (`http://localhost`, `http://127.0.0.1`, `http://[::1]`): Always auto-approved.

### Auto-Update Security

Updates are signed with Ed25519 (minisign format). The public key is embedded in the app binary via `tauri.conf.json`. Signature verification is mandatory and cannot be bypassed — handled by `tauri-plugin-updater` which uses the `minisign_verify` crate. Invalid or missing signatures cause the update to be rejected before installation.

### Binary Download Verification

When downloading `bb` binaries for version mismatches, the accelerator verifies the download against a SHA-256 digest from the GitHub API. If the digest is unavailable or verification fails, the download is rejected (fail-closed). The bundled `bb` sidecar does not require verification.
