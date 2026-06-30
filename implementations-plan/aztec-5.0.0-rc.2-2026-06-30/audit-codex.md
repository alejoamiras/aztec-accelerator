# Codex audit — rc.2 bump plan (xhigh)

Session: 019f19f8-0385-7460-bf83-64c94bee38a2

conditional approve (conditions: prove CI Bun/latest frozen install after lock regen; verify HEAD prove code equals deployed 1.0.6 or test actual 1.0.6 binary; audit bun.lock diff for fresh non-Aztec transitives; require live testnet browser smoke for SponsoredFPC path)

**Security/supply-chain**
- The min-age override is defensible only as an explicit exception, not “small delta.” `bun install --minimum-release-age=0` disables the age gate for the whole resolution, so rc.2 could pull fresh non-`@aztec` transitives. Condition: inspect `bun.lock` diff and publish ages for every new/changed non-`@aztec` package.
- “Local-only” mostly holds: `bunfig.toml` stays unchanged and CI uses `--frozen-lockfile`. But CI uses `bun-version: latest`; local is Bun `1.3.13`. The rc.1 precedent is useful, not conclusive across future Bun behavior. Condition: run frozen install in CI/current latest after lock regen before relying on it.
- npm registry was unreachable here, so I could not verify rc.2 publish time/dist-tags independently.

**Assumptions (Facts/Inferences/Asks)**
- Fact verified: `bunfig.toml` enforces `minimumReleaseAge = 604800`; current pins are only in `packages/sdk/package.json` and `packages/playground/package.json`; accelerator package has no `@aztec` deps.
- Fact overstated: “official packages only” ignores transitives resolved during the override.
- Fact unverified locally: no `accelerator-v1.0.6` tag exists in this checkout. The repo index says 1.0.6 shipped, but I cannot compare HEAD to the deployed binary from a 1.0.6 tag.
- Unsafe inference: P2 e2e builds accelerator from HEAD. There has been heavy accelerator prove/server churn since the last visible accelerator tag (`1.0.5-rc.2`). That e2e proves HEAD compatibility with rc.2, not deployed 1.0.6 compatibility, unless you first prove code parity.
- Unsafe inference: dismissing Noir `!` commits by TS surface alone is too weak. The playground and SDK e2e consume precompiled `SponsoredFPCContract.artifact`; `for_each` order or artifact behavior changes could break fee payment at runtime while `tsc` passes. The local sandbox e2e catches some of this, but not necessarily live testnet deployment/funding/address assumptions.
- Ask to surface: Approach B rejection is operationally plausible but not airtight. Waiting is only clearly rejected if rc.1 is confirmed broken or high-risk against rc.2 testnet; otherwise A is a supply-chain exception for availability.

**General**
- Protocol-version reasoning is directionally right: rc.1 client against rc.2 testnet is a real risk. But local sandbox e2e runs its own rc.2 node, so it does not prove live testnet compatibility, deployed FPC funding, or RPC-specific behavior.
- `tsc --noEmit` is necessary, not sufficient. Keep it, but require accelerated e2e plus live smoke.
- The native e2e’s phase checks are good: it asserts `transmit` and not `fallback`, so it should catch bb CLI/msgpack/proof-format breakage for the binary it runs.

**looks fine**
- SDK-only release shape is reasonable if actual deployed accelerator compatibility is proven.
- Publishing SDK on `testnet`, not `latest`, matches the risk profile.
- `--provenance`, frozen CI installs, exact `@aztec` pins, and manual browser smoke are the right controls.