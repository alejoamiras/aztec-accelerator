## Findings

1. **HIGH — overlong origins can have an unreachable beginning.**  
   [style.css:277](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/src-tauri/frontend/style.css:277) combines `overflow-y:auto` with `justify-content:center`. Once the flex contents exceed the available height, centering distributes overflow above and below the scrollport; the block-start overflow cannot be reached with non-negative `scrollTop`. Since canonicalization imposes no hostname-length limit, a sufficiently long accepted origin can hide its scheme/leading labels permanently. The footer at [style.css:305](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/src-tauri/frontend/style.css:305) does remain reachable. Use start/safe alignment when overflowing.

2. **MEDIUM — the new Playwright test does not test the claimed security properties.**  
   [authorize.spec.ts:95](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/e2e/authorize.spec.ts:95) runs in Playwright’s default large viewport, uses a relatively short origin, and only checks DOM text plus `toBeVisible()`. DOM text still passes when visually clipped or ellipsized, and off-viewport elements are considered visible. It does not assert 400×300 geometry, scroll endpoints, focus, selection, `unicode-bidi`, or punycode/IPv6/private-suffix cases. This test would not catch finding 1.

3. **LOW — shared popup CSS visually regresses the update prompt.**  
   Removing `align-items:center` and `justify-content:center` from the shared container at [style.css:267](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/src-tauri/frontend/style.css:267) also affects [update-prompt.html:11](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/src-tauri/frontend/update-prompt.html:11): its detail box stretches, the checkbox becomes left-aligned, and the group shifts upward. Functionality remains reachable.

4. **LOW — `get_pending_auth` is not actually documented as deferred in the supplied repository.**  
   [C9-plan.md:31](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/implementations-plan/security-hardening/clusters/C9-plan.md:31), [C9-plan.md:54](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/implementations-plan/security-hardening/clusters/C9-plan.md:54), and [C9-plan.md:59](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/implementations-plan/security-hardening/clusters/C9-plan.md:59) still promise it in Phase 1. Only focus-swap and extension validation are listed as deferred.

## What is correct

At [authorize.html:25](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-authorize-popup-safety/packages/accelerator/src-tauri/frontend/authorize.html:25), the origin is real text, focusable, LTR, and labelled. The explicit `user-select:text` overrides the global selection ban, and `unicode-bidi:isolate` applies. Tuple-origin punycode remains canonical because the backend supplies its canonical ASCII serialization and the frontend never decodes it. Remember is genuinely unchecked, with “Always allow this site” and “Allow once” correctly implemented.

Short-origin and recognized-badge layouts remain functional. No new autofocus or selection-based focus stealing was introduced.

The updated tests should themselves pass CI:

- Playwright’s non-exact `"Allow"` accessible-name selector matches “Allow once.”
- WebDriver’s ID selectors are unaffected.
- The persistence test now explicitly checks Remember before Allow.
- The ephemeral test correctly leaves it unchecked.
- Recognized-badge expectations remain valid.

Biome passed. Browser execution was unavailable because this audit sandbox prevents Playwright from writing its transform cache.

## Deferred-risk ranking

1. **MEDIUM — focus-swap/stacked prompts:** highest residual risk because up to ten centered, focused, always-on-top windows can redirect a pending click. Default-unchecked limits persistent compromise, but not accidental one-shot authorization. Acceptable as an explicitly tracked larger windowing change.
2. **LOW — extension validation:** isolation prevents interaction with surrounding text, but bidi controls inside a malformed extension host can still reorder that host. Standard browser extension identifiers are normally ASCII, reducing realistic exposure.
3. **LOW — server-authoritative display binding:** normal production flow passes the same canonical origin and request ID together from the server into the popup URL, so realistic disagreement requires app-internal URL mutation or renderer compromise. Deferral is reasonable for LIGHT scope, but its documentation must be corrected.

VERDICT: changes-requested (HIGH: centered overflow can make the beginning of a long origin unreachable; MEDIUM: the 400x300 reachability test does not exercise or detect that failure)