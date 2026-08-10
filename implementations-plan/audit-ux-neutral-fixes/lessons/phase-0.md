# Phase 0 — de-risk the unknowns

## I-3: does the Rust `--lib` suite run on the Windows and macOS CI legs?

**Answer: YES — and this corrects the plan's own framing of the `NTE_NOT_FOUND` rule.**

Evidence, `.github/workflows/accelerator.yml`:

| Leg | Runner | Runs | Gate |
|---|---|---|---|
| `windows-build` "Windows Build Smoke" (`:590`) | `windows-latest` | `cargo test` (src-tauri, `:608`) **and** `cargo test --manifest-path ../core` (`:612`) | `needs.changes.outputs.desktop == 'true'` |
| `cert-trust` macOS leg (`:188`) | `macos-*` | `cargo test` (src-tauri only) | matrix `platform == 'macos'` |
| `rust-tests` (`:103`) | `ubuntu-latest` | both crates | `desktop == 'true'` |

So a `#[cfg(windows)]` unit test in either crate **does execute on real Windows**. It is not
decoration. `core` is not run on the macOS leg (only `src-tauri` is) — worth knowing, but no item in
this plan puts a macOS-gated test in `core`.

The workflow already records the same lesson one platform over (`:185-187`): *"Until this line NO
workflow ran one on macOS, so every `#[cfg(target_os = "macos")]` unit test … had never compiled
anywhere."*

### What this changes about the plan

The `NTE_NOT_FOUND` rule as written in `plan.md` justifies itself with "cfg-gated code compiles but
never executes". Given the table above, that justification is **too broad and partly wrong** for unit
tests. The accurate version of the lesson is narrower:

> A pure function helps when the decision is a function of inputs a test can **construct**. It does
> not help — and did not fail — where the decision depends on a **real external tool's real output**.

`NTE_NOT_FOUND` was exactly the second case: the constant lived in production code whose only
exercise is a real `certutil` invocation against a real cert store, reachable only from the `#[ignore]`d
real-OS integration suites. No unit test on any platform could have produced that string.

Both plan items survive the correction, with better reasons:

- **F-13**: "which path do we use" is a function of *does the hardcoded file exist* + *env value* —
  both constructible. Pure function is right, and the `#[cfg(windows)]` wrapper test is now also
  available as a real second check on the Windows leg.
- **F-12**: mountinfo content is a constructible string. Pure function is right.

The rule to carry forward is therefore: **prefer purity where inputs are constructible; where a
decision depends on a real tool's real output, no amount of purity substitutes for running it on the
real OS** — which is what the `#[ignore]`d suites are for.

## I-4: `/proc/self/mountinfo` inside an AppImage mount

Not resolved locally — needs a real AppImage run to capture a truthful fixture; a hand-written one
would be exactly the `NTE_NOT_FOUND` mistake (inventing what a real system prints). Options, cheapest
first:

1. Capture it in CI on the Linux leg during an existing AppImage-producing job, print it, copy it into
   the test fixture.
2. Build the AppImage locally once and read it.

Blocking for item 2 only; every other item proceeds. **Do not write the fixture from memory.**
