# Accelerator changelog

## 3.1.0 - Final legacy release

- Presto is the successor: [migration instructions](https://aztec-accelerator.dev/).
- Added a one-time migration notice and permanent **Move to Presto…** tray link. Dismissal is stored
  only in Aztec Accelerator's existing configuration. Proving remains fully functional.
- Added Presto production origins to the recognized-site display registry; authorization still requires consent.
- Retained the existing updater key, native identity, configuration, certificates and installation paths.
  Installed `3.0.0` applications can update normally to `3.1.0`.
- Migration to Presto is manual. Quit this app before starting Presto; nothing installs, copies or
  deletes Presto state automatically. Legacy landing and playground pages now link to Presto.

## 3.0.0 - 2026-09-03

### Breaking change: one manual reinstall is required

Accelerator 3 rotates the updater signing key to public key ID `456E5A3DB518F598`. Existing 1.x and 2.x
installations pin the previous key and therefore cannot authenticate or install a 3.x update through the
in-app updater.

Quit the running accelerator, download the 3.0.0 installer from GitHub Releases, and install it over the
existing application. Do **not** uninstall first unless the normal install-over fails: installing over the
existing app preserves approved sites, settings, HTTPS certificate state, and cached bb versions. After this
one manual reinstall, automatic updates within the 3.x line work normally again.

### Security and release infrastructure

- Replaced the unrecoverable production updater signing key with a freshly generated passwordless keypair.
- Isolated the production private key to the `release-signing` GitHub environment; build and smoke jobs use
  ephemeral keys and never receive it.
- Made SDK publication use npm trusted publishing with GitHub OIDC and provenance verification.
- Added dependency-vulnerability checks as publication gates for both release workflows.
- Hardened release documentation and regression tests around passwordless signing, provenance, promotion,
  and updater-key migration.
- Added a fail-closed updater-baseline resolver that selects the greatest complete published same-key release,
  including prereleases. The completed one-time rotation bootstrap is removed; future key changes require a
  deliberately reviewed migration change and cannot bypass the cross-platform update and tamper suite.
