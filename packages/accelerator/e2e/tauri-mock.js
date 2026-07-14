/**
 * Mock for Tauri IPC — injected via addInitScript BEFORE page scripts load.
 *
 * F-012: the pages now run with `withGlobalTauri: false`, so there is NO `window.__TAURI__`.
 * The bundled `@tauri-apps/api/core` `invoke` delegates to `window.__TAURI_INTERNALS__.invoke(cmd, args)`
 * — that is the seam we mock. We deliberately do NOT define `window.__TAURI__`; the trust-boundary
 * tests assert it stays undefined.
 *
 * Pure JavaScript — no TypeScript syntax. Playwright's addInitScript does not transpile.
 */

// Call counter per command for sequencing support
const callCounts = {};

// Handler registry — supports per-test overrides
const handlers = {};

// Default handlers matching real Rust serde output exactly.
// auto_update is OMITTED (not null) when None in Rust (skip_serializing_if).
const defaults = {
  get_config: () => ({
    config_version: 1,
    safari_support: false,
    approved_origins: ["https://example.com"],
    speed: "full",
    // auto_update intentionally omitted — matches Rust None serialization
  }),
  get_autostart_enabled: () => false,
  get_system_info: () => ({ platform: "macos", cpu_count: 10 }),
  set_speed: () => null,
  set_autostart: () => null,
  set_auto_update: () => null,
  remove_approved_origin: () => null,
  respond_auth: () => null,
  respond_update_prompt: () => null,
  enable_safari_support: () => null,
  disable_safari_support: () => null,
};

window.__TAURI_MOCK__ = {
  calls: [],
  setHandler: (cmd, fn) => {
    handlers[cmd] = fn;
  },
  reset: () => {
    for (const k of Object.keys(handlers)) delete handlers[k];
    for (const k of Object.keys(callCounts)) delete callCounts[k];
    window.__TAURI_MOCK__.calls.length = 0;
  },
};

// The `@tauri-apps/api/core` `invoke` calls `window.__TAURI_INTERNALS__.invoke(cmd, args, options)`.
window.__TAURI_INTERNALS__ = {
  invoke: async (cmd, args) => {
    callCounts[cmd] = (callCounts[cmd] || 0) + 1;
    const callIndex = callCounts[cmd];
    window.__TAURI_MOCK__.calls.push({ cmd, args, callIndex, timestamp: Date.now() });
    const handler = handlers[cmd] || defaults[cmd];
    if (!handler) throw new Error("Unmocked command: " + cmd);
    return handler(args, callIndex);
  },
};
