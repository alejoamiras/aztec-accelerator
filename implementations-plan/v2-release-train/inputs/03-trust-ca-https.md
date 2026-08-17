# Trust / CA / HTTPS lifecycle (agent 3)

Solid (verified): keyless CA never writes signing key to disk, name-constrained to loopback,
fail-closed rotation (certs.rs:399-434), all 3 backends degrade to HTTP on failure. Linux CI leg does
real end-to-end install→verify→remove against certutil; CI runs the real NSIS uninstall hook under
makensis/wine. Above-average rigor.

- **[P0 / BLOCKER] macOS + Linux uninstall never remove the CA; only Windows does.** tauri.conf.json
  :48-56 has no Linux deb postrm and no macOS uninstall hook; nsis/hooks.nsi is Windows-only. Trashing
  the .app or apt remove leaves a permanently-trusted CA in Keychain / ~/.pki/nssdb / Firefox + certs
  on disk. Settings → "Remove certificate trust" works (commands.rs:509-538) but nothing invokes it on
  uninstall for 2 of 3 OSes. Highest priority — install covered on 3 OSes, removal on 1.
  Cost: Linux postrm straightforward; macOS needs a design call (.app has no uninstaller) — document
  the limitation at minimum or ship a removal helper. ~0.5-1.5 days + macOS decision.
- **[P1] Cert expiring while app closed fails silently.** Renewal window only fires while certs_exist()
  && leaf_is_expiring (main.rs:700-718). Once expired, certs_exist() flips false → MissingCertsReset
  (:69-83,:110-113) silently flips https_enabled off with a warn!, no toast/banner. Not an outage (SDK
  degrades) but silently regresses an opted-in feature. Low-med: one-time notice on the reset path.
- **[P2] Consent-dialog install/remove path on macOS+Windows is UNTESTED and the "manual runbook" it's
  deferred to does not exist.** trust_macos/windows.rs run only headless status/verify; comment defers
  the real add-trusted-cert/certutil -addstore flows to "the manual pre-release runbook — do NOT read
  a green here as CI-covered." docs/RELEASE_RUNBOOK.md + PLATFORM_SUPPORT.md contain ZERO mention of
  Keychain/certutil/trust. Most trust-sensitive path on 2/3 platforms has neither automation nor a
  discoverable manual gate. Cost: ~1hr to add checklist now; real automation multi-day.
- **[P3] Windows store-write failure gives uninformative error.** trust/windows.rs:132-147 one generic
  string for declined-dialog vs Group-Policy vs broken certutil, unlike Linux's actionable hint. ~half
  a day.
