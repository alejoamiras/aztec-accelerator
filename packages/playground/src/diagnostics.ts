// ── Crash Diagnostics ──
// Deep runtime instrumentation to capture DataCloneError / OOM at the Worker postMessage boundary.
// Isolated module — safe to remove entirely when no longer needed.
//
// ROLLBACK (frontend):
//   1. Delete this file
//   2. ui.ts — remove `import { diagLog }` and the `diagLog(msg, level)` call in appendLog()
//   3. main.ts — remove diagnostic imports, install*() calls, diagMemory() calls, export btn listener
//   4. index.html — remove the #export-diagnostics-btn button
//
// ROLLBACK (accelerator):
//   1. main.rs — revert to `tracing_subscriber::fmt::init()`, remove log_dir(), Show Logs menu item
//   2. server.rs — revert health() to `json!({ "status": "ok" })`
//   3. Cargo.toml — remove `tracing-appender`, remove `registry` from tracing-subscriber features
//
// Or simply: `git revert <diagnostics-commit>`

interface DiagEntry {
  ts: number;
  type:
    | "log"
    | "memory"
    | "worker-msg"
    | "worker-lifecycle"
    | "wasm-memory"
    | "error"
    | "rejection";
  message: string;
  data?: Record<string, unknown>;
}

// ── Ring buffer (2000 entries) ──

const MAX_ENTRIES = 2000;
const entries: DiagEntry[] = [];

function push(entry: Omit<DiagEntry, "ts">): void {
  if (entries.length >= MAX_ENTRIES) entries.shift();
  entries.push({ ts: Date.now(), ...entry });
}

// ── Size estimation ──

function estimateSize(msg: unknown): number {
  if (msg instanceof ArrayBuffer) return msg.byteLength;
  if (ArrayBuffer.isView(msg)) return (msg as ArrayBufferView).byteLength;
  if (msg == null) return 0;
  try {
    return JSON.stringify(msg).length;
  } catch {
    return 0;
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
}

// ── Worker monkey-patch ──

let workerCount = 0;

export function installWorkerDiagnostics(): void {
  const OriginalWorker = globalThis.Worker;

  globalThis.Worker = class PatchedWorker extends OriginalWorker {
    constructor(scriptURL: string | URL, options?: WorkerOptions) {
      super(scriptURL, options);
      workerCount++;
      push({
        type: "worker-lifecycle",
        message: `Worker created (#${workerCount})`,
        data: { url: String(scriptURL), total: workerCount },
      });
      this.addEventListener("error", (e: ErrorEvent) => {
        push({
          type: "error",
          message: `Worker error: ${e.message}`,
          data: { filename: e.filename, lineno: e.lineno },
        });
      });
    }

    postMessage(msg: unknown, transfer?: Transferable[] | StructuredSerializeOptions): void {
      const size = estimateSize(msg);
      const hasTransfer = Array.isArray(transfer)
        ? transfer.length > 0
        : !!(transfer as StructuredSerializeOptions)?.transfer?.length;
      push({
        type: "worker-msg",
        message: `postMessage ${formatBytes(size)}`,
        data: { sizeBytes: size, hasTransfer },
      });
      try {
        super.postMessage(msg, transfer as any);
      } catch (err) {
        // THIS IS THE ERROR WE'RE HUNTING
        const e = err as Error;
        push({
          type: "error",
          message: `postMessage FAILED: ${e}`,
          data: { sizeBytes: size, errorName: e.name, stack: e.stack },
        });
        captureMemorySnapshot("postMessage-failure");
        throw err;
      }
    }
  };
}

// ── WASM Memory monkey-patch ──

const wasmAllocations: { initial: number; maximum?: number; shared: boolean }[] = [];

export function installWasmDiagnostics(): void {
  const OriginalMemory = WebAssembly.Memory;

  WebAssembly.Memory = class PatchedMemory extends OriginalMemory {
    constructor(descriptor: WebAssembly.MemoryDescriptor) {
      super(descriptor);
      const entry = {
        initial: descriptor.initial,
        maximum: descriptor.maximum,
        shared: !!descriptor.shared,
      };
      wasmAllocations.push(entry);
      push({
        type: "wasm-memory",
        message: `WASM Memory created: ${descriptor.initial} pages (${(descriptor.initial * 64) / 1024}MB)`,
        data: entry,
      });
    }
  };
}

// ── Error handlers ──

export function installErrorHandlers(): void {
  window.addEventListener("error", (e) => {
    const isCritical =
      e.message?.includes("DataCloneError") || e.message?.includes("out of memory");
    push({
      type: "error",
      message: `${isCritical ? "[CRITICAL] " : ""}Uncaught: ${e.message}`,
      data: { filename: e.filename, lineno: e.lineno, colno: e.colno, stack: e.error?.stack },
    });
    if (isCritical) captureMemorySnapshot("critical-error");
  });

  window.addEventListener("unhandledrejection", (e) => {
    const reason = e.reason instanceof Error ? e.reason : String(e.reason);
    const msg = reason instanceof Error ? reason.message : String(reason);
    const isCritical = msg.includes("DataCloneError") || msg.includes("out of memory");
    push({
      type: "rejection",
      message: `${isCritical ? "[CRITICAL] " : ""}Unhandled rejection: ${msg}`,
      data: { stack: reason instanceof Error ? reason.stack : undefined },
    });
    if (isCritical) captureMemorySnapshot("critical-rejection");
  });
}

// ── Memory snapshot ──

function captureMemorySnapshot(label: string): void {
  const mem = (performance as any).memory;
  const jsUsedMB = mem ? +(mem.usedJSHeapSize / (1024 * 1024)).toFixed(1) : undefined;
  const jsTotalMB = mem ? +(mem.totalJSHeapSize / (1024 * 1024)).toFixed(1) : undefined;
  const jsLimitMB = mem ? +(mem.jsHeapSizeLimit / (1024 * 1024)).toFixed(1) : undefined;

  const parts = [`[${label}]`];
  if (jsUsedMB != null) parts.push(`js=${jsUsedMB}/${jsTotalMB}MB limit=${jsLimitMB}MB`);
  parts.push(`workers=${workerCount} wasmInstances=${wasmAllocations.length}`);

  push({
    type: "memory",
    message: parts.join(" "),
    data: {
      label,
      jsUsedMB,
      jsTotalMB,
      jsLimitMB,
      workerCount,
      wasmInstanceCount: wasmAllocations.length,
      wasmAllocations: wasmAllocations.slice(),
    },
  });
}

// ── Export ──

export function downloadDiagnostics(): void {
  const mem = (performance as any).memory;

  const report = {
    exported: new Date().toISOString(),
    userAgent: navigator.userAgent,
    crossOriginIsolated: crossOriginIsolated,
    hardwareConcurrency: navigator.hardwareConcurrency,
    currentMemory: mem
      ? {
          usedMB: +(mem.usedJSHeapSize / (1024 * 1024)).toFixed(1),
          totalMB: +(mem.totalJSHeapSize / (1024 * 1024)).toFixed(1),
          limitMB: +(mem.jsHeapSizeLimit / (1024 * 1024)).toFixed(1),
        }
      : null,
    workerCount,
    wasmAllocations: wasmAllocations.slice(),
    entries,
  };

  const blob = new Blob([JSON.stringify(report, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `aztec-accelerator-diagnostics-${Date.now()}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

// ── Simple wrappers for wiring ──

export function diagLog(msg: string, level: string): void {
  push({ type: "log", message: msg, data: { level } });
}

export function diagMemory(label: string): void {
  captureMemorySnapshot(label);
}
