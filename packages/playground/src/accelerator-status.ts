import type {
  AcceleratorStatus,
  AcceleratorStatusCheckOptions,
} from "@alejoamiras/aztec-accelerator";

export interface AcceleratorStatusView {
  connected: boolean;
  label: string;
  log: string;
  logLevel: "success" | "warn" | "error";
  showInstall: boolean;
  showPermissionHelp: boolean;
}

/** Exhaustive UI model for every public SDK status arm. */
export function acceleratorStatusView(status: AcceleratorStatus): AcceleratorStatusView {
  if (status.available) {
    return {
      connected: true,
      label: "available",
      log: "Native accelerator detected on loopback",
      logLevel: "success",
      showInstall: false,
      showPermissionHelp: false,
    };
  }

  switch (status.reason) {
    case "permission-blocked":
      return {
        connected: false,
        label: "local access blocked",
        log: "Chrome blocked local network access; allow it in Site settings, then Retry",
        logLevel: "warn",
        showInstall: false,
        showPermissionHelp: true,
      };
    case "offline":
      return {
        connected: false,
        label: "not detected, fallback: wasm",
        log: "Accelerator not detected, will fall back to WASM",
        logLevel: "warn",
        showInstall: true,
        showPermissionHelp: false,
      };
    case "error":
      return {
        connected: false,
        label: "health check error, fallback: wasm",
        log: "Accelerator answered unexpectedly; falling back to WASM",
        logLevel: "error",
        showInstall: false,
        showPermissionHelp: false,
      };
    case "version-mismatch":
      return {
        connected: false,
        label: "version mismatch, fallback: wasm",
        log: "Accelerator Aztec version is incompatible; falling back to WASM",
        logLevel: "warn",
        showInstall: false,
        showPermissionHelp: false,
      };
  }
}

interface AcceleratorStatusControllerOptions {
  check: (options?: AcceleratorStatusCheckOptions) => Promise<AcceleratorStatus>;
  render: (status: AcceleratorStatus) => void;
  setPending: (pending: boolean) => void;
}

/**
 * Serializes display ownership without serializing network requests. A newer refresh invalidates an
 * older one's right to render; the SDK itself coalesces same-generation probes.
 */
export class AcceleratorStatusController {
  #epoch = 0;
  #displayed: AcceleratorStatus | null = null;
  #fallbackRefresh: Promise<void> | null = null;

  constructor(private readonly options: AcceleratorStatusControllerOptions) {}

  get displayed(): AcceleratorStatus | null {
    return this.#displayed;
  }

  async refresh(checkOptions?: AcceleratorStatusCheckOptions): Promise<void> {
    const epoch = ++this.#epoch;
    this.options.setPending(true);
    try {
      const status = await this.options.check(checkOptions);
      if (epoch !== this.#epoch) return;
      this.#displayed = status;
      this.options.render(status);
    } finally {
      if (epoch === this.#epoch) this.options.setPending(false);
    }
  }

  /** Fire-and-forget refresh after a native attempt fell back. Never rejects into the proof path. */
  refreshAfterFallback(): void {
    if (!this.#displayed?.available || this.#fallbackRefresh) return;
    this.#fallbackRefresh = this.refresh({ forceRefresh: true })
      .catch(() => {})
      .finally(() => {
        this.#fallbackRefresh = null;
      });
  }
}
