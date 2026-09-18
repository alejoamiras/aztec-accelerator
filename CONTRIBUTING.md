# Contributing

Contributions are welcome. Keep changes focused, explain the user-visible effect, and preserve the
project's fail-closed security and release behavior.

## Repository map

- `packages/sdk`: browser/Node client. It probes the loopback service and falls back to WASM.
- `packages/accelerator/core`: GUI-independent authorization, server, `bb`, cache, and version logic.
- `packages/accelerator/src-tauri`: desktop tray, settings, trust, updater, and OS integration.
- `packages/accelerator/server`: headless CI-only wrapper around the core.
- `packages/playground` and `packages/landing`: project-hosted web applications.
- `.github/workflows` and `docs/RELEASE_RUNBOOK.md`: CI and release contracts.

Before changing a boundary, read [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md). In particular,
origin approval and localhost server authentication are different controls, updater signatures are
independent of Windows installer signing, and upstream `bb` digests are not publisher signatures.

## Set up

Install Bun at the version in `.bun-version` and a current stable Rust toolchain. Desktop builds also
need the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the host OS.

```sh
bun install --frozen-lockfile
```

Do not commit generated build output, local environment files, caches, certificate material, or
secrets.

## Develop and test

Run the smallest relevant test while iterating, then the repository gate before opening a pull
request:

```sh
bun run test
bun run lint:actions
bun run audit:dependencies
```

Useful focused commands include:

```sh
bun run --cwd packages/sdk test:unit
bun run --cwd packages/accelerator test:unit
bun run --cwd packages/accelerator test:e2e:ui
cargo test --locked --manifest-path packages/accelerator/core/Cargo.toml
cargo test --locked --manifest-path packages/accelerator/server/Cargo.toml
cargo test --locked --manifest-path packages/accelerator/src-tauri/Cargo.toml
```

Some OS, packaged-app, updater, and WebDriver tests run only in CI or require explicit local setup.
Describe any check you could not run. Changes to Windows trust, certificates, or the HTTPS listener
also retain the manual Windows composed-proof check documented in the
[desktop README](packages/accelerator/README.md#windows-composed-proof--manual-pre-ga-check).

## Pull requests

- Open an issue first when a change alters a public API, trust boundary, release contract, or
  supported platform behavior.
- Add or update tests for behavior changes. Documentation-only changes should still keep links and
  commands accurate.
- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit subjects.
- Keep lockfile changes intentional and explain new production dependencies.
- Update user-facing documentation and migration notes in the same pull request.
- Do not weaken required CI gates or fail-closed release behavior to make a change pass.

By contributing, you agree that your contribution is licensed under the repository's
[AGPL-3.0-only license](LICENSE). Report vulnerabilities privately under
[SECURITY.md](SECURITY.md), and follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
