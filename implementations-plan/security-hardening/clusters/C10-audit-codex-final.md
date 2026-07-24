VERDICT: CHANGES-REQUESTED

- **D7 — CORRECT** ([C10-plan.md:81](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:81)). IPC transport and Tauri initialization do not require `core:default`; no verified popup flow needs core events/window APIs. Add back only a proven, narrowly scoped permission.

- **D8 — CORRECT** ([C10-plan.md:89](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:89)). Linux built-debug adequately covers the compile-time custom-protocol branch; the three-OS dev matrix still covers platform WebView/IPC differences. All-OS built-debug would be defense-in-depth, not Gate 1 necessity.

- **Test design — MEDIUM false-positive risk** ([C10-plan.md:118](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-tauri-trust-boundary/implementations-plan/security-hardening/clusters/C10-plan.md:118)). An allowed attacker-window command proves the invoke primitive, but not that the forbidden command name is real. A typo/nonexistent command can receive the same ACL denial and leave the canary unchanged. Fix: first successfully invoke the exact negative-target command from its authorized window with valid arguments and verify its effect; then invoke that byte-identical command from the unauthorized window and verify rejection plus unchanged state.

The debug ACL text is not inherently cross-OS brittle because Rust generates it. Match invariant fields—command, attacker label, and `not allowed`/window-context wording—rather than the complete URL or exact full string.

No other confident HIGH/MEDIUM issue.