Changes are required. I found three LOW blockers; no CRITICAL, HIGH, or MEDIUM findings.

Scope note: `c8-gate3.diff` was absent from the worktree, so I audited the complete `7982d6a..HEAD` implementation range plus the current dirty lock/schema deltas.

## Findings

1. **LOW — The preflight does not cover every path actually serialized by the plugin.**

   [commands.rs:57](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/commands.rs:57) validates `std::env::current_exe()`, but:

   - On Linux AppImage, the plugin serializes `app.env().appimage`, not `current_exe`, at [tauri-plugin-autostart/lib.rs:214](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-plugin-autostart-2.5.1/src/lib.rs:214). A newline-bearing AppImage path therefore bypasses [autostart_path_is_safe:138](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:138) and reaches the raw `.desktop` `Exec=` writer at [auto-launch/linux.rs:33](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/linux.rs:33).
   - On macOS, the plugin canonicalizes the executable and inserts it into plist XML without escaping at [tauri-plugin-autostart/lib.rs:202](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tauri-plugin-autostart-2.5.1/src/lib.rs:202) and [auto-launch/macos.rs:90](/home/homelab/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/auto-launch-0.5.0/src/macos.rs:90). The predicate accepts `<` and `&`, which are XML markup hazards—not merely formatting quirks. A crafted directory hierarchy can synthesize closing tags using path separators and inject `Program=/bin/sh`. XML requires literal `<` and `&` in character data to be escaped. [W3C XML §2.4](https://www.w3.org/TR/xml/#syntax)

   Windows `\` and spaces are correctly accepted, but the solution needs platform-specific validation of the exact plugin-selected path, or a properly escaped plugin writer.

2. **LOW — “Fail closed” cleanup and rollback are not implemented reliably.**

   - Unsafe refusal ignores both cleanup results at [commands.rs:60](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/commands.rs:60). A `current_exe()` resolution error returns before either cleanup.
   - Linux cleanup reloads systemd before deleting the unit at [crash_recovery.rs:254](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:254), ignores every failure, deletes afterward, performs no final reload, and always returns `true`.
   - Plugin enable succeeds before the void crash-recovery call at [commands.rs:67](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/commands.rs:67). If crash recovery refuses or fails, the command still returns success with only plugin autostart enabled.
   - The void trait at [crash_recovery.rs:16](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:16) prevents rollback and leaves startup/updater rearm unable to observe failure.

   Therefore stale or half-enabled state remains possible. `enable()` needs a result, `set_autostart` needs transaction-style rollback, and cleanup must remove first, reload afterward, and report failures.

3. **LOW — The advertised validation gate is incomplete and Windows CI will fail.**

   The unconditional test at [crash_recovery.rs:505](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:505) asserts that `/usr/bin/...` is absolute. On Windows it is not—Windows absolute paths require both a prefix and root—while CI runs `cargo test` on Windows at [accelerator.yml:416](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/.github/workflows/accelerator.yml:416). [Rust `Path::is_absolute`](https://doc.rust-lang.org/std/path/struct.Path.html#method.is_absolute)

   Additionally, the “round-trip” test at [crash_recovery.rs:457](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/crash_recovery.rs:457) only exercises a handwritten inverse. It does not validate the production unit, count `ExecStart` directives, or invoke systemd’s parser. It proves sample identity, not independently “no injection.”

   Finally, HEAD omits the required root-package `zeroize` entry in `Cargo.lock` and the regenerated capability schema; both exist only as dirty worktree changes.

## Confirmed correct

- `systemd_exec_start` emits the correct whole-token form `":/path%%"`. Its control/DEL, UTF-8, backslash, quote, and glob rejection matches systemd’s `string_is_safe`; `%` doubling and the `:` prefix correctly neutralize specifier and environment expansion. No additional injection-relevant systemd metacharacter is missing. See systemd’s [`config_parse_exec`](https://github.com/systemd/systemd/blob/main/src/core/load-fragment.c) and [`string_is_safe`](https://github.com/systemd/systemd/blob/main/src/basic/string-util.c).
- Removing `autostart:allow-enable` closes the raw webview command bypass. The retained `allow-disable` and `allow-is-enabled` permission names are correct and do not break the Rust-backed toggle.
- F-016 is technically correct: `&Zeroizing<KeyPair>` deref-coerces to `&KeyPair`, all error paths zeroize on drop, and explicit drop occurs immediately after signing. The residual accurately limits the guarantee to rcgen’s serialized DER. The only stale wording is [certs.rs:139](/home/homelab/Projects/aztec-accelerator/aztec-accelerator-2/.claude/worktrees/sechard-desktop-platform-secrets/packages/accelerator/src-tauri/src/certs.rs:139), which still says the CA key is dropped “at function end.”
- No cfg/dead-code problem is apparent. Formatting and diff whitespace checks passed; the three existing Linux crash-recovery tests passed from the current test binary.

VERDICT: changes-requested (3 LOW findings)