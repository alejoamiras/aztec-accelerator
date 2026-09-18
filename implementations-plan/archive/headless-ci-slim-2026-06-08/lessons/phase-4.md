# Phase 4 — docs + drift note

- **Release-tarball runtime behavior** is documented in-code (`server/main.rs` comment, core-extraction Phase 2):
  `AZTEC_BB_VERSION` unset → `"unknown"`; `/prove` unaffected (callers pass `x-aztec-version`). External CI users
  who want a truthful `/health.aztec_version` export `AZTEC_BB_VERSION`.
- **Known drift (deferred):** the desktop apt list now lives in 3 places — `setup-accelerator` desktop-branch +
  headless-branch (`libssl-dev` only) + `_e2e.yml:49`. `_e2e.yml`'s WebKit/GTK is ALSO waste (it only builds the
  headless server) — a clean follow-up would route `_e2e.yml` through the composite with the slim flags. Out of
  THIS scope (e2e untouched per owner).
- **`release-accelerator.yml` `build-headless`** still carries the same waste → needs an rc dry-run → separate
  follow-up (out of scope per "PR-gate CI, no rc").
- Phase 3b complete.
