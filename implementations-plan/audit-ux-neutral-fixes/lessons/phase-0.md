# Phase 0 — experiments before design

## I-3 (from rev 1): does the Rust `--lib` suite run on Windows / macOS CI?

**Answer: YES.** `.github/workflows/accelerator.yml`:

| Leg | Runner | Runs |
|---|---|---|
| `windows-build` (`:590`) | `windows-latest` | `cargo test` src-tauri (`:608`) **and** core (`:612`) |
| `cert-trust` macOS (`:188`) | `macos-*` | `cargo test` src-tauri only |
| `rust-tests` (`:103`) | `ubuntu-latest` | both crates |

A `#[cfg(windows)]` unit test is **not** decoration. One reviewer claimed the Windows leg runs only
the `--test trust_*` integration suites; it read the `cert-trust` matrix and missed the separate
`windows-build` job.

### What this corrected about the plan

Rev 1 justified the "everything must be pure" rule with "cfg-gated code compiles but never executes".
Given the table, that is **wrong for unit tests**. The accurate lesson, which both auditors converged
on independently:

> Purity buys testability of **decision logic**. It buys nothing for **platform constants, env-var
> names, error codes, and path formats** — which is exactly where `cfg`-gated bugs live.

`CRYPT_E_NOT_FOUND` was a wrong constant *value*; a pure function handed that same wrong constant
catches nothing anywhere. Only real Windows found it.

---

## Experiment 1 — real `/proc/self/mountinfo` from a genuine AppImage

**Method**: downloaded our own released `Aztec-Accelerator-1.0.7-Linux-x86_64.AppImage` (`gh release
download accelerator-v1.0.7`), mounted it with `--appimage-mount`, captured `/proc/self/mountinfo`
while mounted, unmounted. Deliberately not hand-written — inventing what a real system prints is the
`NTE_NOT_FOUND` mistake.

**The real line:**

```
76 47 0:68 / /home/homelab/.cache/tmp/.mount_Aztec-doOHJf ro,nosuid,nodev,relatime shared:417 \
  - fuse.Aztec-Accelerator-1.0.7-Linux-x86_64.AppImage Aztec-Accelerator-1.0.7-Linux-x86_64.AppImage \
  ro,user_id=1000,group_id=1000
```

Contrast from the same capture:

```
47 1 252:0 / / rw,relatime shared:1 - ext4 /dev/mapper/ubuntu--vg-ubuntu--lv rw
54 47 7:0 / /snap/canonical-livepatch/406 ro,nodev,relatime shared:88 - squashfs /dev/loop0 ro,…
```

### Result: this REFUTES the design rev 3 adopted

Rev 3 reversed rev 2 to adopt codex's design: *bind the mount source to canonical `$APPIMAGE`*.
**The mount source is the BASENAME, not the path** — `Aztec-Accelerator-1.0.7-Linux-x86_64.AppImage`,
with no directory component. There is nothing to compare a canonical absolute `$APPIMAGE` against, so
the design cannot be implemented as specified. A basename comparison is satisfied by any attacker who
names their payload identically.

Two review rounds argued about this design; one experiment settled it. **Cost: ~3 minutes.** This is
the argument for Phase 0 existing at all.

### What the sample DOES support — the rule now adopted

`fstype` is the discriminator, and it is clean:

| Path | fstype | Passes a `fuse.` test? |
|---|---|---|
| genuine AppImage mount | `fuse.<AppImage basename>` | **yes** |
| `/` | `ext4` | no |
| `/usr`, `/home`, `/opt` (normal) | not a mount, or `ext4`/`xfs` | no |
| snap mounts | `squashfs` | no |

**Rule**: *find the mountinfo entry whose mountpoint is canonical `$APPDIR`; its fstype must start with
`fuse.`; our exe must live under that mountpoint.*

This is **strictly stronger than either audited design**:
- vs. rev 2's two-`stat` mountpoint test — that could not tell a fuse mount from a real `/usr`
  partition, so it left the split-`/usr` bypass open. `fuse.` closes it.
- vs. rev 3's mount-source binding — which cannot be built at all.

Bonus check available at no cost: the fstype subtype carries the AppImage's basename, so
`subtype == basename($APPIMAGE)` catches an `$APPDIR` inherited from a *different* AppImage.

**Honest residual**: an attacker who both controls our environment and names their payload identically
to ours still passes. Unfixable from mountinfo. Such an actor can already write
`~/.config/autostart/*.desktop` directly, so the marginal gain is nil.

**Incidental finding**: the mountpoint was `~/.cache/tmp/.mount_Aztec-doOHJf`, not `/tmp/...`, because
the AppImage runtime honours `$TMPDIR`. Any rule keying on a `/tmp` prefix would be wrong. Nothing in
the plan does, but it was close.

---

## Experiment 2 — `GetExtendedTcpTable` on CI (item 7)

Cannot be run locally (no Windows host). Deliberately **not** landed as a throwaway spike: the spike
code and the production code are the same FFI call, so the "spike" is written as the permanent
regression test and shipped early in the stack to get its CI answer while later phases proceed.

Test shape: open a listener on an ephemeral port in-process, resolve its owning PID via
`GetExtendedTcpTable`, assert it equals `std::process::id()`, resolve the image path and assert it
matches `current_exe()`. If it cannot see a same-user socket without elevation, item 7 reverts to
deferred per the approved goal.
