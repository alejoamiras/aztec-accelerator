# Accelerator changelog

## 3.0.0 - Unreleased

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
- Added a fail-closed updater-baseline resolver and a narrowly constrained first-RC key-rotation bootstrap;
  the next RC must return to the full same-key cross-platform update and tamper suite before GA.
