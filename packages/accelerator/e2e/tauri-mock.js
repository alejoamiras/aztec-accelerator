/**
 * Mock for window.__TAURI__ — injected via addInitScript BEFORE page scripts load.
 * tauri-bridge.js destructures window.__TAURI__.core on line 9 during <head>,
 * so this MUST be installed before any page navigation.
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
    https_enabled: false,
    approved_origins: ["https://example.com"],
    speed: "full",
    onboarding_version: 1,
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
  enable_https: () => null,
  disable_https: () => null,
  get_trust_status: () => ({
    stores: [{ store: "macOS Keychain", installed: true, detail: null }],
  }),
  remove_https_trust: () => null,
  get_onboarding_state: () => ({
    platform: "macos",
    https_default: true,
    autostart_enabled: false,
    auto_update: null,
    trust_status: { stores: [] },
  }),
  // Default: everything succeeds and the marker is set.
  complete_onboarding: () => ({
    https: { Ok: null },
    autostart: { Ok: null },
    auto_update: { Ok: null },
    completed: true,
  }),
  dismiss_onboarding: () => null,
  open_onboarding: () => null,
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

window.__TAURI__ = {
  core: {
    invoke: async (cmd, args) => {
      callCounts[cmd] = (callCounts[cmd] || 0) + 1;
      const callIndex = callCounts[cmd];
      window.__TAURI_MOCK__.calls.push({ cmd, args, callIndex, timestamp: Date.now() });
      const handler = handlers[cmd] || defaults[cmd];
      if (!handler) throw new Error("Unmocked command: " + cmd);
      return handler(args, callIndex);
    },
  },
  event: {
    listen: async () => () => {},
    emit: async () => {},
  },
  // Minimal window API for pages that close themselves (onboarding wizard). Records close() calls so
  // specs can assert the window was dismissed.
  window: {
    getCurrentWindow: () => ({
      close: async () => {
        window.__TAURI_MOCK__.calls.push({ cmd: "__window.close", callIndex: 1 });
      },
    }),
  },
};
