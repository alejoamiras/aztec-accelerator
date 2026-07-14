# C3 plan-audit — Fable leg. Verdict: conditional approve
Conditions (folded): (1) dtolnay/rust-toolchain — @stable IS the channel selector; SHA-pin the
action (master history, not the GC-able @stable commit) AND add `with: toolchain: stable`
(_e2e.yml + setup-accelerator/action.yml). (2) 3 implicit-`latest` setup-bun sites in credentialed
workflows (deploy-landing, publish-testnet, publish-nightlies) + no repo bun source of truth
(local 1.3.14) → central `packageManager: bun@1.3.14`; also pin `node-version: 24`→24.18.0.
(3) Phase-1 gate regex too weak (@v[0-9] misses @stable/@main/short-SHA) → require @[0-9a-f]{40}.
(4) actionlint: don't run remote installer; download exact release + verify SHA-256; verify cache-hit
bytes too. (5) shellcheck `&& || echo` MASKS failures → `find infra -type f -name '*.sh' -print0 |
xargs -0r shellcheck` (recursive, matches infra/**/*.sh filter). (6) no dependabot → add github-actions
updater w/ nested-composite `directories`, or documented cadence. Survey VERIFIED complete; local `./`
refs correctly excluded. Residuals: setup-aztec curls install.aztec.network unverified; publish-* grant
contents/id-token globally (→ C5); pinning != vetting.
