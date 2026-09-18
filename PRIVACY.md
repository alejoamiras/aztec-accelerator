# Privacy Notice

**Last updated:** 2026-09-03

Aztec Accelerator is designed to perform proving locally. The project has no user accounts and does
not add product analytics, advertising trackers, remote crash reporting, or application telemetry
to the SDK, desktop app, landing site, or playground.

## Private proving data

The SDK sends a private witness only to the configured loopback Accelerator endpoint, or processes
it in the SDK's WASM fallback. The desktop/headless server writes the witness to an owner-only local
temporary workspace so the local `bb` process can prove it. The workspace is normally removed when
the request ends; crash residue is cleaned conservatively on a later successful server start when
safe to do so.

Witness input is not intentionally included in project logs or sent to a project-operated server.
`bb` diagnostics, local paths, and approved origin names can appear in local logs, so review and
redact logs before sharing them. Browser prover instances are HTTPS-only by default: no private
proving payload and no `/prove` request is sent over HTTP. After HTTPS fails, the SDK may make one
bounded, witness-free HTTP `GET /health` request solely to improve recovery guidance. The response
does not make HTTP eligible for proving.

The accepted localhost impersonation boundary is documented in the [security
model](docs/SECURITY_MODEL.md#2-accept-unauthenticated-http-discovery-and-explicit-plaintext-proving).
Node/Bun/SSR clients remain HTTP-compatible for the headless CI server, and a browser dApp can offer
HTTP after an informed, current-session confirmation. In either plaintext mode, another local
process can impersonate a stopped Accelerator on the fixed HTTP port and receive a witness. The SDK
does not persist browser consent; the dApp must not store it in local storage, cookies, URL
parameters, or desktop configuration.

## Data stored on your device

The desktop app stores local operational data, including:

- settings and approved origins in the user's `.aztec-accelerator/config.json`;
- verified `bb` binaries and integrity markers in `.aztec-accelerator/versions/`;
- local HTTPS certificates and private keys in `.aztec-accelerator/certs/` when enabled;
- updater and ownership/recovery state under `.aztec-accelerator/`; and
- up to seven daily rolling application logs plus a separate `panic.log` in the OS-local
  application data directory listed in the [desktop README](packages/accelerator/README.md#logs).

This state remains on the device unless a feature explicitly makes a network request. The normal
uninstaller intentionally preserves configuration and cached `bb` versions so upgrades and
reinstalls retain user choices. The desktop README explains how to remove certificate trust and OS
integration before deleting the app. Remaining `.aztec-accelerator` state and log files can then be
deleted manually by the user.

The playground keeps a dismissed-banner preference and `bb.js` proving cache/version markers in
browser storage. Current playground wallet state and any confirmed HTTP fallback are in memory; the
HTTP choice is reset on reload and is never written to browser or desktop storage. Cleanup code
removes legacy Aztec wallet/PXE databases from older visits. Clear site data in the browser to remove
playground storage.

## Network requests and service providers

Depending on the component and feature, the software may contact:

- GitHub's API and release asset service for Accelerator releases, upstream AztecProtocol `bb`
  releases, and their published digests;
- the project website and updater feed served through AWS S3/CloudFront;
- the npm registry when installing the SDK; and
- the configured Aztec node when using the playground or an integrating dApp.

Those providers receive routine request metadata such as IP address, user agent, requested URL, and
time under their own terms and privacy policies. GitHub also processes issue, pull-request, and
security-report interactions made through its service. The project does not combine that provider
metadata into an application analytics profile.

If you email a security or conduct report, the information you provide is used to investigate and
respond to that report and is retained only as needed for that purpose or a legal obligation.

Questions about this notice can be sent to
[alejo@aztec.foundation](mailto:alejo@aztec.foundation).
