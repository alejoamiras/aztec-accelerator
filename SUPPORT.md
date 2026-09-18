# Support

## Where to ask

Use [GitHub Issues](https://github.com/alejoamiras/aztec-accelerator/issues) for reproducible bugs,
installation problems, documentation gaps, and feature requests. Search existing issues first and
keep one problem per issue.

For a bug, include:

- whether it affects the SDK, desktop app, headless CI server, landing site, or playground;
- the exact app/package version and installation method;
- operating system, architecture, browser, and Aztec version where relevant;
- expected and actual behavior;
- minimal reproduction steps; and
- the smallest useful log excerpt or screenshot.

Review logs before posting them. They may contain approved origin names, local paths, and `bb`
diagnostics. Never post private witnesses, wallet material, credentials, access tokens, signing
material, or unrelated personal data.

Suspected security vulnerabilities must be reported privately using [SECURITY.md](SECURITY.md), not
through an issue. Conduct concerns use the private contact in
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Support policy

Support is best effort and has no response-time SLA. The maintained targets are the current stable
desktop release and npm `latest`; upgrade before requesting a fix for an older version when
possible. The headless server is supported only for ephemeral, single-tenant CI runners.

Start with the component documentation:

- [Desktop installation and troubleshooting](packages/accelerator/README.md)
- [SDK API and browser behavior](packages/sdk/README.md)
- [Platform support](docs/PLATFORM_SUPPORT.md)
- [Security model](docs/SECURITY_MODEL.md)

General Aztec protocol, node, wallet, or `bb` questions belong in the corresponding Aztec project
unless Accelerator is what introduced the problem.
