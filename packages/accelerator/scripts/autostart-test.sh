#!/usr/bin/env bash
# L4 — hermetic autostart env proofs, in Docker (plan §6). Local-only, like test:nsis; NOT a CI job.
#
# What only a container can prove (the dev host can't — HOME and XDG_CONFIG_HOME coincide there):
#   1. D9: the owned autostart module watches XDG_CONFIG_HOME, not a hardcoded $HOME/.config — the
#      harness passes DIVERGENT dirs and the test asserts the decoy stays untouched.
#   2. C8: with HOME and XDG unset entirely, every operation degrades to Err/Skip — the removed
#      auto-launch crate PANICKED here (`dirs::home_dir().unwrap()`).
#
# The crate is compiled INSIDE the container (a host-built test binary would need the host's glibc).
# First run is slow (image ~2 GB of tauri build deps + a cold cargo build); later runs reuse the
# named volumes below and finish in seconds-to-a-minute.
#
#   bun run --cwd packages/accelerator test:autostart
set -euo pipefail

PKG_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="aztec-autostart-hermetic"

if ! docker info >/dev/null 2>&1; then
  echo "docker is required (hermetic env divergence needs a container); the lifecycle test runs natively in CI" >&2
  exit 1
fi

if [ ! -f "$PKG_DIR/src-tauri/frontend/assets/.build-manifest.json" ] || \
   ! ls "$PKG_DIR"/src-tauri/binaries/bb-* >/dev/null 2>&1; then
  echo "build.rs preconditions missing — run: bun run --cwd packages/accelerator frontend:build && bun run --cwd packages/accelerator prebuild" >&2
  exit 1
fi

# rust:1-bookworm + the exact tauri dep list setup-accelerator installs in CI. Cached after run one.
docker build -q -t "$IMAGE" - >/dev/null <<'DOCKERFILE'
FROM rust:1-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libgtk-3-dev \
      pkg-config && \
    rm -rf /var/lib/apt/lists/*
DOCKERFILE

# -i is required: without stdin attached, `bash -s` reads an empty script and exits 0 — a green run
# that tested nothing (test:nsis lesson). Named volumes keep the cargo registry + target warm.
docker run --rm -i \
  -v "$PKG_DIR":/src:ro \
  -v aztec-autostart-cargo-registry:/usr/local/cargo/registry \
  -v aztec-autostart-target:/target \
  -e CARGO_TARGET_DIR=/target \
  "$IMAGE" bash -s <<'RUNNER'
set -euo pipefail

# The crate tree must be writable (build.rs emits into OUT_DIR relatives) — copy out of the RO mount.
mkdir -p /work
cp -r /src/src-tauri /work/src-tauri
cp -r /src/core /work/core
cd /work/src-tauri

echo "── build (cold runs take minutes; the /target volume keeps later runs warm) ──"
cargo test --test autostart_heal --no-run --quiet

echo "── leg 1: divergent XDG_CONFIG_HOME vs HOME (D9) ──"
mkdir -p /h /xdg
env AZTEC_AUTOSTART_HERMETIC=1 HOME=/h XDG_CONFIG_HOME=/xdg \
  cargo test --test autostart_heal --quiet linux_hermetic_xdg_divergence -- --ignored --nocapture

# The decoy assertion also holds OUTSIDE the process: nothing may exist under /h at all beyond
# the lock dir the module itself owns.
if find /h -name '*.desktop' | grep -q .; then
  echo "FAIL: a .desktop landed under the \$HOME decoy — the module is not honouring XDG" >&2
  exit 1
fi

echo "── leg 2: no HOME, no XDG at all (C8 — the removed plugin panicked here) ──"
env -u HOME -u XDG_CONFIG_HOME AZTEC_AUTOSTART_HERMETIC=1 \
  cargo test --test autostart_heal --quiet linux_hermetic_no_home_is_graceful -- --ignored --nocapture

echo "OK — XDG divergence honoured, HOME-less runs degrade gracefully"
RUNNER
