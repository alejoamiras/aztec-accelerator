# C3 (action-pinning / F-015) — audit trail

## GATE 1 dual plan-audit (Codex + Fable): both conditional approve → folded
See clusters/C3-audit-codex.md + C3-audit-fable.md (merged with C3). Key catches: rust-toolchain@stable-is-channel-selector (would break CI); 3 implicit-latest bun sites in credentialed workflows; actionlint installer mislabel (1.7.11→1.7.10); shellcheck failure-masking; missing dependabot.

## GATE 3 post-impl audit: CHANGES-REQUIRED → folded
```
Verdict: **CHANGES-REQUIRED**

Blocking finding:

- Three `setup-bun` sites still float to `latest`: [deploy-landing.yml](.github/workflows/deploy-landing.yml:21), [publish-nightlies.yml](.github/workflows/publish-nightlies.yml:56), and [publish-testnet.yml](.github/workflows/publish-testnet.yml:65). They omit `bun-version`, while root [package.json](package.json:1) has no `packageManager`/`engines.bun`; pinned setup-bun therefore falls back to `latest` ([upstream behavior](https://github.com/oven-sh/setup-bun/blob/0c5077e51419868618aeaa5fe8019c62421857d6/README.md#L11-L15)). Add `bun-version: 1.3.14` at all three.

Everything else passes:

- All 123 remote `uses:` refs are valid 40-hex SHAs with accurate `# vN`/`# master` comments; all resolve to the stated upstream refs. All 35 local `./` refs remain unpinned. Cache/save/restore correctly share `caa296…`.
- Both Rust sites use the master SHA plus valid `with: toolchain: stable`; indentation/YAML is correct.
- All explicit Bun `latest` values became `1.3.14`; both Node sites became `24.18.0`.
- [actionlint.yml](.github/workflows/actionlint.yml:53) correctly pins 1.7.12, verifies the correct Linux-amd64 SHA-256, extracts top-level `actionlint`, and invokes `./actionlint -color`. Cache removal leaves no dangling dependencies. ([v1.7.12 release](https://github.com/rhysd/actionlint/releases/tag/v1.7.12))
- Shellcheck is empty-safe; a lint failure propagates as nonzero (`xargs` returns 123), failing the Bash step.
- [dependabot.yml](.github/dependabot.yml:5) is schema-valid; `directories` supports globs and the composite-action pattern covers all four action directories. ([GitHub documentation](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference))
- Actionlint 1.7.12 passed every workflow; all 28 `.github` YAML files parsed; no other new regression found.

Audited the clean target worktree at `e65001a`; the outer supplied cwd is checked out on another branch.```

Blocking finding folded: 3 setup-bun sites (deploy-landing/publish-nightlies/publish-testnet) OMITTED bun-version → still floated to latest → added bun-version: 1.3.14. Everything else approved (123 valid SHAs, rust fix, actionlint verified-download, shellcheck, dependabot).
