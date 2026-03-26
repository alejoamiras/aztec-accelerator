# Platform Support

## Supported Platforms

| Platform | Architecture | Status | Notes |
|----------|-------------|--------|-------|
| **macOS 13+** (Ventura) | Apple Silicon (arm64) | Supported | Primary development platform |
| **macOS 13+** (Ventura) | Intel (x86_64) | Supported | Built and tested in CI |
| **Linux** | x86_64 | Supported | .deb and .AppImage provided |
| **Windows** | — | Not supported | No build target configured |

## macOS Details

- **Code-signed and notarized** via Apple Developer ID
- **Auto-update** via Ed25519-signed artifacts (tauri-plugin-updater)
- **Safari support** available: local HTTPS server with auto-generated CA certificate trusted via Keychain
- **System tray** app — no Dock icon by default (Accessory activation policy)
- **Start on Login** via LaunchAgent plist with crash recovery (KeepAlive + ThrottleInterval)

### Safari HTTPS Support

Safari blocks mixed content (HTTP from HTTPS pages). The accelerator can generate a local CA certificate, trust it in the macOS Keychain (requires password), and run an HTTPS server on port 59834. Enable via Settings > Safari Support.

The CA certificate is constrained to `127.0.0.1`, `::1`, and `localhost` via X.509 Name Constraints. Leaf certificates are auto-renewed when expiring within 30 days.

## Linux Details

- **.deb** package for Debian/Ubuntu-based distros
- **.AppImage** for other distros (self-contained, no install needed)
- **System tray** requires a tray implementation (most desktop environments provide one; Wayland compositors may vary)
- **Crash recovery** via systemd user service with `Restart=on-failure`
- **No HTTPS support** — Safari is not available on Linux. Firefox and Chrome on Linux handle localhost HTTP correctly.

### Wayland

The app uses GTK via WebKitGTK. Tray icon support depends on the compositor:
- GNOME: requires an extension (e.g., AppIndicator/KStatusNotifierItem)
- KDE Plasma: works out of the box
- Sway/wlroots: requires `waybar` or similar with tray support

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
