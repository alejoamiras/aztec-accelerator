# Platform Support

## Supported Platforms

| Platform | Architecture | Status | Notes |
|----------|-------------|--------|-------|
| **macOS 13+** (Ventura) | Apple Silicon (arm64) | Supported | Primary development platform |
| **macOS 13+** (Ventura) | Intel (x86_64) | Supported | Built and tested in CI |
| **Linux** | x86_64 | Supported | .deb and .AppImage provided |
| **Windows** | x86_64 | Supported (unsigned installer) | Per-user NSIS installer; SmartScreen shows Unknown publisher |

## Encrypted Connection (HTTPS)

HTTPS between the browser and the accelerator is **default-on**, consented through the first-run
onboarding wizard, on **all three** desktop OSes (it was previously a macOS-only "Safari Support"
toggle). It gives an encrypted, authenticated loopback channel; Safari *requires* it (Safari blocks
plain HTTP from an HTTPS page), while Chrome/Firefox/Edge use it when the local certificate is trusted
and report an actionable secure-connection failure when it is not. Browser proving never activates
HTTP automatically.

The certificate is a **keyless local CA** (the CA signing key is generated in memory, signs one
`localhost` leaf, and is discarded — never written to disk, so the trusted anchor can mint nothing)
constrained to `127.0.0.1`, `::1`, and `localhost` via X.509 Name Constraints. The leaf is auto-renewed
within 30 days of expiry.

| OS | Trust store | Consent | Rotation re-trust | Uninstall |
|----|-------------|---------|-------------------|-----------|
| **macOS** | login Keychain (`security`) | password dialog on install | renewal consent window → password | Settings "Remove certificate trust"; or Keychain Access |
| **Windows** | CurrentUser `Root` (`certutil.exe`) | the wizard's *Start* click (no separate dialog is guaranteed) | renewal consent window | NSIS uninstaller removes it; or Settings "Remove certificate trust" |
| **Linux** | user NSS DBs — `~/.pki/nssdb` (Chrome/Chromium/Brave/Edge) + each Firefox profile — via `certutil` | the wizard's *Start* click (no OS dialog exists) | silent (user DBs need no auth) | Settings "Remove certificate trust"; or `AztecAccelerator --remove-ca-trust` |

**Linux notes.** Requires `certutil` (the `.deb` depends on `libnss3-tools`; the AppImage detects it and
degrades with an install hint if absent). Per-store trust status is shown honestly in the wizard/Settings.
**Sandboxed (snap/flatpak) Chromium keeps a private, confined trust store the app cannot reach** — it is
disclaimed, not silently claimed as covered. Firefox must be restarted to pick up a newly added anchor.

**Full uninstall (all OSes).** The trust-store column above is only the CA. A complete teardown also
removes the autostart entry and the crash-recovery task/unit. **Windows** does all of it in the NSIS
uninstaller (guarded so an in-place upgrade never fires it). **macOS/Linux** have no uninstall hook, so run
`AztecAccelerator --prepare-uninstall` (or `packages/accelerator/scripts/uninstall.sh`) before deleting the
app. It is **ownership-checked**: a second, copied install that still shares `~/.aztec-accelerator` leaves
all shared state intact and reports why. Config and approved origins are never removed by any path.

## macOS Details

- **Code-signed and notarized** via Apple Developer ID
- **Auto-update** via Ed25519-signed artifacts (tauri-plugin-updater)
- **Encrypted connection (HTTPS)** via the login Keychain — see [Encrypted Connection (HTTPS)](#encrypted-connection-https) above (this is what Safari requires)
- **System tray** app — no Dock icon by default (Accessory activation policy)
- **Start on Login** via LaunchAgent plist with crash recovery (KeepAlive + ThrottleInterval)

## Windows Details

- **NSIS installer** (per-user, `installMode: currentUser`). It is not Authenticode-signed, so first install shows Windows SmartScreen's **Unknown publisher** warning. This is a known distribution limitation; users must choose **More info → Run anyway**.
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

### Browser Local Network Access (Chrome 142+, Firefox 153+)

Chrome 142 began gating requests from public websites to loopback addresses behind a user permission prompt (Local Network Access); Chrome 145 splits it into `local-network` and `loopback-network` permissions. Firefox 153 enables its corresponding Local Network Access protection by default for desktop users. A dApp probing the accelerator from a public origin triggers the browser prompt on first use. An explicit denial is reported by the SDK as `permission-blocked` and still falls back to WASM; a pending/dismissed prompt or unavailable permission query can remain inconclusive.

The SDK annotates supported plaintext requests with `targetAddressSpace: "loopback"`. This declares the destination so supporting browsers can run their LNA flow; it does not bypass permission. HTTPS is not an escape hatch because the gate follows address space, not scheme. Site permissions beside the address bar are the usual recovery, followed by a forced Retry, but this is not guaranteed: enterprise policy can require an administrator, while an iframe can require top-level access or explicit Permissions Policy delegation.

Keep this flow separate from HTTPS recovery. If HTTPS cannot connect, the SDK reports
`secure-connection-unavailable` with a best-effort diagnosis. The normal repair is Accelerator tray
→ Settings → enable **Encrypted Connection**, or re-run certificate setup for trust failures, then
force Retry. The SDK may use one witness-free HTTP health diagnostic after HTTPS failure, but never
sends an HTTP `/prove` or witness automatically. Safari may block that diagnostic, leaving the
diagnosis `unconfirmed`; its trusted local HTTPS path remains required. Requests from
`localhost`-served pages (local dev) are same-address-space and do not trigger the public-to-loopback
prompt.

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
