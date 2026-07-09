# Aztec Accelerator

Native proving accelerator for Aztec transactions. Bypasses browser WASM throttling by running the `bb` proving binary natively on your machine, exposed via a localhost HTTP server that the SDK auto-detects.

If every dApp in the ecosystem uses `AcceleratorProver` with accelerated mode, a single install of this app gives users native-speed proving across all of them — no per-app setup, no downside.

[![Accelerator](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml/badge.svg)](https://github.com/alejoamiras/aztec-accelerator/actions/workflows/accelerator.yml)

> **dApp developer?** You're looking for the [SDK package](../sdk/README.md) — `npm install @alejoamiras/aztec-accelerator` gives your app native proving with zero user-side configuration.

## Installation

Download the latest release from [GitHub Releases](https://github.com/alejoamiras/aztec-accelerator/releases):

| Platform | Format |
|----------|--------|
| macOS (Apple Silicon) | `.dmg` |
| macOS (Intel) | `.dmg` |
| Linux (x86_64) | `.deb`, `.AppImage` |

**Running CI tests?** The release also ships a [headless server tarball](#headless-server-for-ci-test-acceleration) for accelerating end-to-end tests on GitHub-hosted runners.

### Upgrading from 1.0.1 (macOS)

> **If you previously auto-updated from 1.0.0 to 1.0.1 on macOS and your app now hangs on launch**, the in-place bundle swap broke `amfid`'s signature cache. **1.0.2 fixes the underlying cause** but cannot fix your already-broken install — you need a manual reinstall:
>
> 1. Force-quit any stuck `aztec-accelerator` processes (Activity Monitor → `×`).
> 2. Drag `/Applications/Aztec Accelerator.app` to Trash. Empty Trash.
> 3. Download the **1.0.2 DMG** from the latest release and install fresh.
>
> Your config at `~/.aztec-accelerator/config.json` (approved origins, speed, auto-update preference) is preserved across reinstall.
>
> See the [1.0.2 release notes](https://github.com/alejoamiras/aztec-accelerator/releases/tag/accelerator-v1.0.2) for the full root-cause writeup.

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

The default port is `59833`. The SDK reads `AZTEC_ACCELERATOR_PORT` to override the client-side target. The server itself currently does **not** honor this env var — it always binds `127.0.0.1:59833`. If you need to change the port on both sides, that requires a code change to `server.rs`.

### Automatic Version Management

The accelerator automatically downloads and caches `bb` binaries on demand. When the SDK sends a prove request for an Aztec version the accelerator doesn't have yet, it downloads the correct binary from Aztec's GitHub releases, caches it, and uses it immediately.

Cached binaries are stored in `~/.aztec-accelerator/versions/` with a retention policy per network tier:

| Tier | Example | Kept |
|------|---------|------|
| Nightly | `5.0.0-nightly.20260309` | 2 |
| Devnet | `5.0.0-devnet.20260309` | 3 |
| Testnet | `5.0.0-rc.2` | 5 |
| Mainnet | `5.0.0` | all |

Old versions are evicted automatically — no manual cleanup needed.

### Version Model — why an Aztec bump doesn't re-release this app

Because the accelerator downloads `bb` at runtime (above), it is **decoupled from the Aztec protocol version**. When Aztec ships a new release, only the [SDK](../sdk/README.md) is republished — it carries the `@aztec/*` deps and advertises its version via the `x-aztec-version` header. The **already-installed accelerator** (desktop *and* headless) fetches and caches the matching `bb` on the next prove request; users do nothing. You cut a new accelerator release only when the accelerator's **own** code changes (server, tray, updater, or the `bb` download/verification logic) — never merely to track an `@aztec` version bump.

### bb Binary Resolution

When no specific version is requested, the accelerator looks for the `bb` binary in this order:

1. **`BB_BINARY_PATH` env var** — explicit override (CI, testing)
2. **Sidecar** — bundled with the app (`binaries/bb`)
3. **`~/.bb/bb`** — user-installed via the Aztec CLI
4. **`PATH`** — system-wide installation

## Site Authorization

The accelerator uses a MetaMask-style approval flow. When a new website calls `/prove`, the user sees a popup asking to allow or deny access. **Localhost origins are prompted once too** (then remembered if you choose **Remember**) — the desktop app no longer silently auto-approves localhost, so a malicious local page can't quietly use the accelerator. (The headless CI server *does* auto-approve localhost — it's an operator-controlled environment; see the [Headless Server section](#headless-server-for-ci-test-acceleration).)

- **Allow + Remember**: the origin is saved to `~/.aztec-accelerator/config.json` and never prompted again
- **Allow** (without Remember): approved for this session only
- **Deny**: the SDK receives a `403` and automatically falls back to WASM proving
- **Timeout** (60s): auto-denied if the user doesn't respond

Approved sites can be reviewed and removed from the Settings window.

For the headless server binary (CI/testing), set `ALLOWED_ORIGINS=origin1,origin2` to restrict browser-driven access. See the [Headless Server section](#headless-server-for-ci-test-acceleration) below for the full security model.

## Headless Server for CI Test Acceleration

> **For CI test acceleration only. Not for production. Not for shared or self-hosted CI runners.**
>
> The headless server is a standalone binary — no tray, no window, and (since the core extraction) **no Tauri / WebKit / GTK** in its dependency tree. It exists so external Aztec dApp teams can install the accelerator on their CI runners and speed up E2E test proving (native `bb` instead of WASM). Built from the GUI-agnostic `accelerator-core` crate, the Linux tarball has no desktop dependencies — on GitHub-hosted `ubuntu-latest` it runs out of the box.
>
> **Security caveats — read before deploying:**
>
> - The server listens on `127.0.0.1` only, AND (SEC-01a) every request must carry a loopback `Host`/`:authority` (`127.0.0.1` / `localhost` / `[::1]`) on the listener port — this closes the DNS-rebinding vector where a remote web page rebinds its domain to loopback. A forged/non-loopback `Host` is rejected with `403 invalid_host` before any route logic.
> - **Deny-by-default (SEC-01c):** with `ALLOWED_ORIGINS` unset the server now **gates** browser origins and **denies any non-localhost origin** (localhost/`127.0.0.1`/`[::1]` stay auto-approved). Set `ALLOWED_ORIGINS=a,b` to pre-approve specific origins, or pass `--allow-all` / `ACCEL_ALLOW_ALL=1` to opt back into approving every origin (mutually exclusive with `ALLOWED_ORIGINS` → fails loud). Unset no longer means "approve everyone".
> - Non-browser callers on the **same host** (curl, another local process) can still reach `/prove` (loopback `Host`, no `Origin`) — inherent to a localhost service. Acceptable for single-tenant CI but **unsafe for shared/self-hosted runners or any production environment**.
> - The tarball is shipped with a SHA-256 sidecar only, not a cryptographic signature. Verify the checksum before extracting.
> - Do not run this as a service. Do not expose it on a public interface. Do not use on a multi-tenant host.

### Download

Each [GitHub release](https://github.com/alejoamiras/aztec-accelerator/releases) ships four tarballs:

| Platform | Tarball |
|---|---|
| macOS (Apple Silicon) | `accelerator-server-${VERSION}-macos-arm64.tar.gz` |
| macOS (Intel)         | `accelerator-server-${VERSION}-macos-x86_64.tar.gz` |
| Linux (x86_64)        | `accelerator-server-${VERSION}-linux-x86_64.tar.gz` |
| Linux (ARM64)         | `accelerator-server-${VERSION}-linux-arm64.tar.gz` |

Each has a matching `.sha256` sidecar file.

### Install in GitHub Actions (Linux x86_64 example)

```yaml
- name: Install aztec-accelerator headless server
  env:
    ACCELERATOR_VERSION: "1.0.6"
  run: |
    BASE_URL="https://github.com/alejoamiras/aztec-accelerator/releases/download/accelerator-v${ACCELERATOR_VERSION}"
    TARBALL="accelerator-server-${ACCELERATOR_VERSION}-linux-x86_64.tar.gz"
    curl -sSfL "${BASE_URL}/${TARBALL}" -o "${TARBALL}"
    curl -sSfL "${BASE_URL}/${TARBALL}.sha256" -o "${TARBALL}.sha256"
    shasum -a 256 -c "${TARBALL}.sha256"
    tar -xzf "${TARBALL}"
    sudo mv accelerator-server /usr/local/bin/

- name: Start headless accelerator
  env:
    ALLOWED_ORIGINS: http://localhost:5173
  run: accelerator-server > accelerator.log 2>&1 &
```

The accelerator will download the matching `bb` binary on the first prove request (from Aztec's GitHub releases) and cache it in `~/.aztec-accelerator/versions/`. Subsequent runs reuse the cache.

### Configuration

| Env var | Effect |
|---|---|
| `ALLOWED_ORIGINS` | Comma-separated browser origins pre-approved for `/prove`. **Unset = deny-by-default** (non-localhost denied; localhost auto-approved). Mutually exclusive with `--allow-all` / `ACCEL_ALLOW_ALL`. |
| `ACCEL_ALLOW_ALL` | `1` or `true` → approve **all** browser origins (the pre-SEC-01 behavior). Opt-in; mutually exclusive with `ALLOWED_ORIGINS`. Prefer `ALLOWED_ORIGINS` for an explicit allowlist. (`--allow-all` CLI flag is equivalent.) |
| `BB_BINARY_PATH` | Path to a pre-installed `bb` binary, bypassing the auto-download. |
| `RUST_LOG` | Standard `tracing-subscriber` filter (e.g. `info`, `debug`). |

### Verifying it's running

```sh
curl http://127.0.0.1:59833/health
# {"status":"ok","api_version":1,"version":"...","aztec_version":"...","available_versions":[...],"bb_available":true}
```

### Source

The headless server is its own Cargo crate at `packages/accelerator/server/`, separate from the desktop `src-tauri/` crate so that `tauri build` cannot pick it up and stowaway it into the desktop `.app` bundle (the root cause of the 1.0.1 auto-update breakage). It depends on the GUI-agnostic **`accelerator-core`** crate (`packages/accelerator/core/`) via a path dep to reuse the shared `server`, `authorization`, `config`, `bb`, and `versions` modules — with **none** of the Tauri / WebKit dependency tree. That core extraction is what lets the headless build stay desktop-dependency-free.

Build it with:

```sh
cargo build --release --manifest-path packages/accelerator/server/Cargo.toml
```

The binary lands at `packages/accelerator/server/target/release/accelerator-server`. The entry point is `packages/accelerator/server/src/main.rs` — same logic as the embedded server in the desktop app, just bootstrapped without Tauri's main loop.

## Tray Menu

The tray menu adapts based on the build profile:

**Production** (release builds):
```
Settings
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
  Settings
─────────────
v1.1.0 · Aztec 5.0.0-nightly.20260309
GitHub
Quit
```

### Settings Window

Click **Settings** in the tray menu to open the Settings window. From here you can:

- **Approved Sites** — view and remove origins that have been granted access
- **Start on Login** — auto-launch at login (LaunchAgent on macOS, autostart on Linux)
- **Auto-Update** — toggle automatic updates on or off
- **Safari Support** (macOS only) — toggle HTTPS mode for Safari compatibility
- **Proving Speed** — control CPU usage with a 5-level slider (Low / Light / Balanced / High / Full)

Speed changes take effect immediately on the next prove request — no restart needed.

### Auto-Update

The accelerator checks for updates on launch and every 12 hours. Updates are signed with Ed25519 and verified before installation.

On the first update, you'll see a prompt:
- **Update Now** — downloads, installs, and restarts immediately
- **Remind Me Later** — dismisses the prompt (it returns next launch)
- **Keep me updated automatically** — checkbox that enables silent future updates

With auto-update enabled, new versions are downloaded and installed in the background — the app restarts seamlessly. You can change this anytime in Settings.

### Safari Support (macOS only)

Safari blocks mixed content — an HTTPS page cannot `fetch()` from `http://127.0.0.1`. Chrome and Firefox exempt localhost, but Safari does not.

To enable, toggle **Safari Support** in the Settings window. macOS will prompt for your password once to trust the certificate.

The SDK automatically probes both HTTP (59833) and HTTPS (59834) in parallel via `Promise.any`. Chrome/Firefox use HTTP (faster), Safari uses HTTPS.

**What it installs:** A local Certificate Authority (`Aztec Accelerator Local CA`) with Name Constraints limiting it to `127.0.0.1`, `::1`, and `localhost` only. The CA is installed in your macOS login Keychain.

**To remove:** Open **Keychain Access**, search for "Aztec Accelerator Local CA", delete it, then disable Safari Support in Settings.

**Certificate details:**
- CA: ECDSA P-256, 10-year validity, Name Constraints (localhost only)
- Leaf: ECDSA P-256, 824-day validity (one day under Apple's inclusive 825-day TLS cap), auto-renewed
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

# Run Rust tests (~90 tests)
cargo test

# Build release bundle (.dmg / .deb / .AppImage)
cargo tauri build

# Quick-run the production menu locally (release build, no bundling)
cargo run --release
```

## Testing

### Rust unit tests (~90)
```bash
cargo test --manifest-path packages/accelerator/src-tauri/Cargo.toml
```

### Playwright UI mock tests (28)
Tests the Settings, Authorization, and Update Prompt windows with mocked Tauri IPC:
```bash
bun run --cwd packages/accelerator test:e2e:ui
```

### WebDriver E2E tests (9)
Real end-to-end tests that launch the actual Tauri app via `tauri-plugin-webdriver` and drive it with WebdriverIO. Covers smoke (app health), settings (speed persistence), and auth flow (Allow/Deny/Remember).

```bash
# Terminal 1: launch app with WebDriver
cargo tauri dev --features webdriver

# Terminal 2: run tests
bun run --cwd packages/accelerator test:e2e:webdriver
```

These run on both macOS and Linux in CI as a PR gate (`accelerator.yml`) and pre-release gate (`release-accelerator.yml`).

### Cross-version download test
Tests the full bb binary download pipeline (HTTP → SHA-256 → extract → cache). Gated behind `ACCELERATOR_DOWNLOAD_TEST=1`:
```bash
ACCELERATOR_DOWNLOAD_TEST=1 cargo test download_and_verify -- --nocapture
```

## Release Pipeline

Releases are triggered via `gh workflow run release-accelerator.yml -f version=X.Y.Z`.

```
validate → e2e-webdriver gate → tag → build (3 Tauri + 4 headless platforms) → post-build smoke → release → bump
```

- **E2E gate**: builds with `--features webdriver`, runs 9 WebDriver tests (macOS, release mode)
- **Build**: Tauri bundles for 3 platforms (macOS arm64/x86_64, Linux x86_64) + headless `accelerator-server` for 4 platforms (macOS arm64/x86_64, Linux x86_64/arm64)
- **Post-build smoke**: mounts the signed DMG, launches the app, polls `/health`
- **Release**: creates GitHub Release with DMGs/debs/AppImages + headless tarballs (with SHA-256 sidecars) + `latest.json` for auto-updater
- **Bump**: auto-creates PR to bump source version
