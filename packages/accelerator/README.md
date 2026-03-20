# Aztec Accelerator

Native proving accelerator for Aztec transactions. Bypasses browser WASM throttling by running the `bb` proving binary natively on your machine, exposed via a localhost HTTP server that the SDK auto-detects.

If every dApp in the ecosystem uses `AcceleratorProver` with accelerated mode, a single install of this app gives users native-speed proving across all of them — no per-app setup, no downside.

[![Accelerator](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml)

## Installation

Download the latest release from [GitHub Releases](https://github.com/alejoamiras/aztec-accelerator/releases):

| Platform | Format |
|----------|--------|
| macOS (Apple Silicon) | `.dmg` |
| macOS (Intel) | `.dmg` |
| Linux (x86_64) | `.deb`, `.AppImage` |

### macOS Gatekeeper

The app is code-signed and notarized by Apple. It should open without any Gatekeeper warnings. If macOS still blocks it (e.g., a local build), allow it via:

1. Open **System Settings → Privacy & Security**
2. Scroll to the "Security" section
3. Click **Open Anyway** next to the Aztec Accelerator message

### Linux

**Wayland tray icon limitation:** Tauri's system tray does not render on GNOME Wayland ([tauri-apps/tauri#14234](https://github.com/tauri-apps/tauri/issues/14234)). The `.deb` package includes a workaround that forces the X11 GDK backend via the `.desktop` file, so the tray icon appears correctly out of the box.

If you use the `.AppImage` on Wayland and the tray icon is missing, launch with:

```sh
GDK_BACKEND=x11 ./aztec-accelerator.AppImage
```

For a tray-only app with no visible window, X11 mode has zero downsides.

## How It Works

The accelerator runs as a **menu bar / system tray app** with no window — just a tray icon with a status menu.

When running, it listens on `http://127.0.0.1:59833` for proving requests from the SDK. The flow:

```
Browser (SDK)  →  HTTP POST /prove  →  Accelerator  →  bb binary  →  proof
                  (localhost:59833)     (Tauri app)     (native)
```

The SDK auto-detects the accelerator on port 59833. If the accelerator is unavailable or has a version mismatch, the SDK automatically falls back to WASM proving.

### Proving Timing

Every `/prove` response includes an `x-prove-duration-ms` header with the actual `bb` proving time in milliseconds. The SDK surfaces this via the `"proved"` phase callback, and the frontend displays it in the step breakdown — making it easy to see how much time is pure proving vs. network/serialization overhead.

## Configuration

### Port

The default port is `59833`. Override it with the `AZTEC_ACCELERATOR_PORT` environment variable (must match on both SDK and accelerator sides).

### Automatic Version Management

The accelerator automatically downloads and caches `bb` binaries on demand. When the SDK sends a prove request for an Aztec version the accelerator doesn't have yet, it downloads the correct binary from Aztec's GitHub releases, caches it, and uses it immediately.

Cached binaries are stored in `~/.aztec-accelerator/versions/` with a retention policy per network tier:

| Tier | Example | Kept |
|------|---------|------|
| Nightly | `5.0.0-nightly.20260309` | 2 |
| Devnet | `5.0.0-devnet.20260309` | 3 |
| Testnet | `5.0.0-rc.1` | 5 |
| Mainnet | `5.0.0` | all |

Old versions are evicted automatically — no manual cleanup needed.

### bb Binary Resolution

When no specific version is requested, the accelerator looks for the `bb` binary in this order:

1. **`BB_BINARY_PATH` env var** — explicit override (CI, testing)
2. **Sidecar** — bundled with the app (`binaries/bb`)
3. **`~/.bb/bb`** — user-installed via the Aztec CLI
4. **`PATH`** — system-wide installation

## Tray Menu

The tray menu adapts based on the build profile:

**Production** (release builds):
```
Start on Login
─────────────
v1.1.0 · Aztec 5.0.0-nightly.20260309
GitHub
Quit
```

**Development** (debug builds via `cargo tauri dev`):
```
Status: Idle
▸ Versions
  Show Logs
  Start on Login
─────────────
v1.1.0 · Aztec 5.0.0-nightly.20260309
GitHub
Quit
```

### Auto-Start on Login

When enabled, the accelerator launches automatically when you log in to your computer. Uses platform-native mechanisms (LaunchAgent on macOS, autostart on Linux).

### Safari Support (macOS only)

Safari blocks mixed content — an HTTPS page cannot `fetch()` from `http://127.0.0.1`. Chrome and Firefox exempt localhost, but Safari does not.

To fix this, enable **Safari Support** from the tray menu:

1. Click **☐ Safari Support** in the tray menu
2. A dialog explains what will happen — click **Continue**
3. macOS will ask for your password to trust the certificate (one-time setup)
4. The accelerator starts an HTTPS listener on port **59834**

The SDK automatically probes both HTTP (59833) and HTTPS (59834) in parallel via `Promise.any`. Chrome/Firefox use HTTP (faster), Safari uses HTTPS.

**What it installs:** A local Certificate Authority (`Aztec Accelerator Local CA`) with Name Constraints limiting it to `127.0.0.1`, `::1`, and `localhost` only. The CA is installed in your macOS login Keychain.

**To remove:** Open **Keychain Access**, search for "Aztec Accelerator Local CA", delete it, then disable Safari Support in the tray menu.

**Certificate details:**
- CA: ECDSA P-256, 10-year validity, Name Constraints (localhost only)
- Leaf: ECDSA P-256, 825-day validity (Apple TLS maximum), auto-renewed
- Storage: `~/.aztec-accelerator/certs/`

## Version Compatibility

The accelerator supports multiple Aztec versions simultaneously. The `/health` endpoint reports the bundled version and all cached versions:

```json
{
  "status": "ok",
  "version": "1.1.0",
  "aztec_version": "5.0.0-nightly.20260309",
  "available_versions": ["5.0.0-nightly.20260309", "5.0.0-nightly.20260308"],
  "bb_available": true
}
```

When the SDK requests a version that isn't cached, the accelerator downloads it automatically. If the download fails, the SDK falls back to WASM proving.

## Troubleshooting

### Logs

The accelerator writes daily-rotating logs. Open the log directory from the tray menu (**Show Logs**) or find them at:

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/aztec-accelerator/logs/` |
| Linux | `~/.local/share/aztec-accelerator/logs/` |

### Port Conflicts

If port 59833 is already in use, the accelerator will fail to start. Check for conflicts:

```sh
lsof -i :59833
```

### bb Binary Not Found

If no `bb` binary is found, the `/health` endpoint returns `"bb_available": false` and `/prove` requests return a 500 error. The accelerator will attempt to download the binary automatically when a versioned prove request arrives. To install manually:

```sh
curl -s https://install.aztec.network | bash
aztec install
```

## Development

```sh
# Prerequisites: Rust toolchain, Tauri CLI
cargo install tauri-cli

# Copy bb binary for sidecar (reads version from @aztec/bb.js)
bun run --filter accelerator prebuild

# Run in development mode (debug build — includes Versions + Show Logs in menu)
cd packages/accelerator/src-tauri
cargo tauri dev

# Run tests
cargo test

# Build release bundle (.dmg / .deb / .AppImage)
cargo tauri build

# Quick-run the production menu locally (release build, no bundling)
cargo run --release
```
