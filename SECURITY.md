# Security Policy

## Supported versions

Security fixes are provided on a fix-forward basis for:

| Component | Supported line |
|---|---|
| Desktop app | Current stable GitHub release |
| TypeScript SDK | Version on npm's `latest` dist-tag |
| Prereleases | Best effort while actively being evaluated |
| Older releases and dist-tags | No guaranteed fixes |

When practical, update to the latest stable version before reporting. A security fix may require a
new release rather than a patch to an older line.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability. Email
[alejo@aztec.foundation](mailto:alejo@aztec.foundation) with the subject
`[aztec-accelerator security]` and include:

- the affected component and version;
- operating system and browser or runtime, where relevant;
- impact and the assumptions required to reproduce it;
- minimal reproduction steps or a proof of concept; and
- any suggested mitigation.

Do not include real private witnesses, credentials, signing keys, tokens, or third-party personal
data. Use synthetic test data and encrypt especially sensitive supporting material before sending
it; ask for a preferred transfer method in the initial email.

This is a solo-maintained project, not a staffed security desk. The target is to acknowledge a
complete report within three business days and provide an initial assessment within seven business
days, but these are best-effort targets rather than an SLA. Please allow a reasonable remediation
and release window before disclosure.

## Scope

Reports may cover the SDK, desktop app, headless CI server, updater/release path, and project-hosted
landing or playground code. Vulnerabilities in Aztec, `bb`, GitHub, npm, AWS, Tauri, browsers, or
other dependencies should also be reported to their respective maintainers when the defect is
upstream.

The project has explicitly documented accepted boundaries for upstream `bb` publisher trust,
localhost service discovery, the unsigned Windows installer, and the CI-only headless server. Read
the [security model](docs/SECURITY_MODEL.md) before reporting one of these cases. A bypass, broader
impact, or materially different precondition is still in scope and should be reported privately.

Good-faith research that avoids privacy violations, service disruption, destructive actions, and
access to data that is not yours is welcome. No bug bounty or payment program is currently offered.
