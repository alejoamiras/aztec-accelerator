**1. Phased plan**

1. Safety-first prep PR. Before moving files, make version/build metadata explicit instead of crate-local. Right now `/health.version` comes from `env!("CARGO_PKG_VERSION")` inside the shared server module ([server.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/server.rs:159)), while `accelerator-server --version` comes from the headless binary crate ([server main](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/src/main.rs:28)). If `server.rs` moves unchanged into a new crate, `/health` will silently report the core crate’s version. Fix that first by passing `app_version` through `HeadlessState`/`AppState`. Do the same review for `AZTEC_BB_VERSION`, which is currently injected by [src-tauri/build.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/build.rs:1).

2. Extraction PR. Create `packages/accelerator/core` as a plain path dependency package, not a workspace member. Move only the headless-needed modules: `authorization`, `bb`, `config`, `versions`, `server` HTTP path, and the shared state/callback types. Keep the GUI crate as the top layer and re-export core modules from [src-tauri/src/lib.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/lib.rs:1) so most GUI imports stay stable. Change [server/Cargo.toml](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/server/Cargo.toml:21) to depend on `accelerator-core`, not `aztec-accelerator`.

3. GUI-only follow-up PR. Leave `certs`, `commands`, `verified_sites`, `updater`, `crash_recovery`, and Tauri-facing code in `src-tauri`. Add a tiny GUI-local HTTPS adapter that wraps `core::router(...)` and owns `tokio-rustls`/`hyper-util`, replacing the current `crate::server::start_https` call sites in [main.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:55) and `commands.rs`. That keeps Safari support behavior identical without polluting core.

4. CI PR. Split desktop and headless setup paths. `build-headless` currently goes through the desktop setup composite and still installs Linux WebKit/GTK deps and runs Bun prebuild intended for Tauri ([release workflow](/Users/alejoamiras/Projects/aztec-accelerator/.github/workflows/release-accelerator.yml:237), [setup action](/Users/alejoamiras/Projects/aztec-accelerator/.github/actions/setup-accelerator/action.yml:62)). After extraction, headless should use Rust-only setup plus any genuinely required native libs.

5. Validation PR / rc dry-run. Run the existing Rust tests, WebDriver/e2e, release workflow dry-run, and keep the current macOS bundle invariant in place. The key proof is unchanged behavior plus a smaller headless graph.

**2. Q2 verdict**

Pick **(b)**: `accelerator-core` should be **headless-needs-only**, with `certs` and `verified_sites` left in the GUI crate.

Reasoning:

- `verified_sites` is GUI-only today: it is consumed by `commands.rs` and loaded in [main.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/src/main.rs:242). Moving it buys no headless value and drags build-script questions with it.
- `certs` is not just “Tauri-free Rust.” It is part of the Safari/macOS trust story and tied to current build-script responsibilities in [build.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/build.rs:12).
- The highest-risk item is feature unification. A feature-gated single core can be made safe today, but only because there is **no shared Cargo workspace** right now. I would not encode “headless stays lean” as a convention when it can be made structural.

I would also keep the TLS accept loop out of core. It is technically GUI-agnostic, but headless never uses it. A small GUI-local adapter is cheaper than carrying feature complexity forever.

**3. CI restructure and measurement**

CI approach:

- Keep desktop and headless as separate package builds.
- Add `install-tauri-system-deps: false` and `run-prebuild: false` inputs to the setup composite, or split it into `setup-accelerator-desktop` and `setup-accelerator-headless`.
- Do not introduce a shared `[workspace]` just to “share cache.” With the current package layout, that is more likely to create feature bleed than real value.
- Keep the current version self-report assertion for the binary. Make `/health.version` independent of core crate version so the workflow does not need to patch a third manifest.

Measurement:

- Baseline now: `cargo tree -p accelerator-server --prefix none | sort -u | wc -l` is **475** unique package lines in this checkout, and the tree currently includes `tauri`, `tauri-build`, `tauri-plugin-*`, `rcgen`, `rustls`, and `tokio-rustls`.
- Add a dedicated Linux benchmark job that runs cold on `ubuntu-latest` with cache disabled:
  - `/usr/bin/time -f '%e' cargo build --release --manifest-path packages/accelerator/server/Cargo.toml --target x86_64-unknown-linux-gnu`
  - `cargo tree --manifest-path packages/accelerator/server/Cargo.toml -p accelerator-server --prefix none | sort -u | wc -l`
  - `cargo tree ... | rg '^(tauri|tauri-|rcgen|rustls|tokio-rustls)'` must be empty after the split.

**4. Adversarial findings**

- Library extraction and macOS bundle topology: **confirm, narrowly**. Adding a library crate does not add a new executable to the `.app`. The GUI package still has `autobins = false` and one `[[bin]]` ([Cargo.toml](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/Cargo.toml:16)). So the `Contents/MacOS` executable topology should stay unchanged. That does **not** mean “skip validation”: build scripts, sidecars, or bundle resources can still drift, so the existing bundle-contents invariant and updater smokes remain mandatory.
- Feature unification footgun: today, no workspace means no cross-package unification. `cargo metadata` shows `server` and `src-tauri` each as their own workspace root. If you later add a shared workspace, `resolver = "2"` is helpful but **not sufficient**. A workspace build that includes both crates can still unify `accelerator-core/tls`. `default-features = false` on server also does **not** protect against another package enabling the feature in the same build graph. The strong mitigation is: **keep no shared workspace between GUI and headless**.
- Cyclic dependency risk: easy to create if core reaches back into `commands`, `certs`, or `updater`. Core must be strictly downward-only. GUI can re-export core; core must never import GUI.
- Version-stamp integrity: this is the biggest concrete bug in the brief. `/health.version` is currently tied to the crate that compiles `server.rs`, not to the headless binary crate. If you miss that, the extraction ships a misleading version surface.
- Supply chain: the headless tarball is unsigned; the `.sha256` sidecar is integrity, not authenticity. Extraction still narrows trust surface meaningfully because headless can stop compiling Tauri and, if CI is cleaned up properly, stop running Bun prebuild / npm-side bb setup at all.

**5. Assumption attacks**

- Misstated/incomplete fact: “core candidates” omitted the load-bearing build-script seam. [src-tauri/build.rs](/Users/alejoamiras/Projects/aztec-accelerator/packages/accelerator/src-tauri/build.rs:1) is part of the extraction problem.
- Unsafe inference: “separate core-crate caching” is not naturally available under the current non-workspace, per-target-dir setup. The real CI win is removing desktop-only setup from headless.
- Ask to surface: should `/health.version` report GUI/server release version, core crate version, or something else? It is currently ambiguous and wrong for a moved shared module.
- Ask to surface: is `AZTEC_VERSION` allowed to become a shared committed input for headless builds, or must headless CI keep running the Bun prebuild to refresh it from the SDK dependency tree? That changes provenance and should be explicit.