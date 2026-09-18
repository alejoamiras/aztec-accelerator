# Security Model

**Status:** accepted project policy

**Last reviewed:** 2026-09-03

Aztec Accelerator moves private proving work from browser WASM to a native `bb` process on the
same machine. This document states what the project protects, where it depends on other systems,
and which residual risks are consciously accepted.

## System and trust boundaries

The normal proving path is:

```text
dApp -> SDK -> loopback HTTP/HTTPS server -> local bb process -> proof
```

- The SDK is application code in the dApp's browser or Node/Bun process.
- The desktop app owns the loopback server, approval UI, local `bb` cache, certificate material,
  and updater. The headless server reuses the proving core but is a CI-only operator tool.
- `bb` is published by AztecProtocol. Accelerator downloads it from the upstream
  `aztec-packages` GitHub releases and does not build or publish it.
- Desktop application releases are published by this project. macOS artifacts are signed and
  notarized. Windows first-install packages are intentionally unsigned. All updater payloads are
  Ed25519-signed and verified independently of OS package signing.

The main confidential asset is the private witness sent to `/prove`. Approved-origin state,
locally generated TLS private keys, updater state, and cached executable integrity metadata are
also treated as private or security-sensitive local state.

## Security goals and controls

The project is designed to:

- bind the proving service to loopback and reject non-loopback host authorities;
- require a remembered user approval before a desktop browser origin may prove, including
  localhost origins;
- keep witness workspaces and sensitive state owner-only, clean normal workspaces after proving,
  and reap abandoned workspaces conservatively after a later successful start;
- validate connection configuration so the SDK cannot be redirected away from loopback;
- default browser proving to HTTPS, pin HTTPS after it succeeds, and never send an HTTP `/prove` or
  witness unless the dApp obtains explicit session consent and opts in;
- verify downloaded `bb` bytes against upstream release digests, re-hash cached executables before
  use, and fail closed on a missing or invalid marker for a requested version; and
- accept only correctly signed updater payloads and reject updater rollback or artifact confusion.

These controls reduce risk within the boundaries below; they do not turn those boundaries into
cryptographic guarantees.

## Accepted decisions

### 1. Depend on upstream `bb` publisher security

The runtime obtains both a `bb` release asset and its SHA-256 digest from AztecProtocol's GitHub
release infrastructure. The comparison detects transit corruption, incomplete downloads, cache
modification, and a release asset that changes after a digest was recorded. It does **not** provide
independent publisher authentication if the upstream publisher account or the shared GitHub
control plane is compromised.

The project deliberately depends on AztecProtocol to introduce a publisher signature or
attestation. It will not maintain a parallel private signing scheme, binary mirror, or claim that
manual digest review proves upstream authorship. This dependency is tracked in
[#343](https://github.com/alejoamiras/aztec-accelerator/issues/343).

When upstream publishes a stable signing or attestation mechanism, adoption requires a deliberately
reviewed change: pin the upstream identity or key, verify the statement in CI and at runtime where
applicable, define migration behavior for older releases, and preserve fail-closed execution. Until
then, the current digest and cache checks remain defense in depth under the explicit upstream-trust
assumption.

### 2. Accept unauthenticated HTTP discovery and explicit plaintext proving

The SDK discovers the service on fixed loopback ports and accepts a `/health` response only when it
matches the Accelerator health contract. This is a shape check, **not server authentication**.
Browser page and Web Worker clients use HTTPS for private proving by default. If that connection
fails, the SDK may make one bounded, witness-free HTTP `GET /health` request to produce a recovery
diagnosis. The diagnostic never serializes or transmits a witness, never POSTs to HTTP, never pins
HTTP, and cannot make the endpoint eligible for proving.

The HTTP diagnosis can be forged by any local process that binds the fixed port, so its output is
only best-effort repair guidance. A detailed recognized response distinguishes HTTPS disabled from
an advertised-but-unreachable HTTPS listener; privacy-limited health can only report that Accelerator
appears reachable; failed, blocked, malformed, and foreign responses are unconfirmed.

Node, Bun, and SSR retain an HTTP-compatible default for the single-tenant headless CI use case. A
browser dApp may also expose an explicit current-session escape hatch. In either case, if plaintext
proving is enabled while the real Accelerator is stopped, a hostile local process—including one
running as another user on a multi-user machine—can imitate `/health` and receive a witness sent to
`/prove`. Origin approval protects the real service from browser sites; it cannot make an impostor
enforce the same policy. A same-user compromise is already able to access the user's processes and
files and is outside the localhost-service threat model.

Browser plaintext proving must be an informed, non-persisted choice on the current prover instance:

```ts
prover.setAcceleratorConfig({
  httpsOnly: false,
  allowInsecureDowngrade: true,
});
```

The SDK does not store this choice; dApps must not copy it to local storage, cookies, URL parameters,
or desktop configuration. Reloading or constructing another browser prover restores HTTPS-only.
The original live proof of the plaintext boundary and the evaluated alternatives are recorded in the
[independent hardening report](../audit/security/2026-08-21-independent-hardening/report.md#ih-sec-1--low-inherent-design-boundary--port-squat-witness-capture).

### 3. Ship an unsigned Windows first installer

The Windows NSIS first installer is not Authenticode-signed, so SmartScreen displays **Unknown
publisher**. This is an accepted distribution trade-off. It does not relax the updater boundary:
subsequent updater payloads must pass the embedded Ed25519 verification before installation.

### 4. Restrict the headless server to single-tenant CI

The headless server has no approval UI or desktop TLS lifecycle. It is supported only as an
ephemeral CI test accelerator on a single-tenant runner. Shared runners, long-running services,
public listeners, and production deployment are outside its supported threat model.

## Not accepted

The decisions above do not authorize:

- binding the proving server to a non-loopback interface;
- bypassing host or origin validation in the desktop app;
- sending a witness to a non-loopback SDK destination;
- installing an unsigned or incorrectly signed updater payload;
- executing a requested cached `bb` version whose integrity marker is absent or invalid; or
- representing a same-origin digest as proof of an uncompromised upstream publisher.

Report behavior outside these documented boundaries through [SECURITY.md](../SECURITY.md).
