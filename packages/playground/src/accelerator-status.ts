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

type LoopbackPermissionName = "loopback-network" | "local-network-access";
type LoopbackPermissionStatus = {
  readonly state: PermissionState;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
};
type LoopbackPermissions = {
  query(descriptor: { name: LoopbackPermissionName }): Promise<LoopbackPermissionStatus>;
};

/**
 * Observe an already-open browser LNA prompt without keeping the bounded health check alive. Chrome
 * can leave the fetch pending until our header deadline, then resolve the permission minutes later;
 * the PermissionStatus change is the only event that tells the page to start a fresh probe.
 */
export async function watchLoopbackPermissionChanges(
  onChange: (state: PermissionState) => void,
): Promise<() => void> {
  let permissions: LoopbackPermissions | undefined;
  try {
    if (typeof navigator === "undefined") return () => {};
    permissions = navigator.permissions as unknown as LoopbackPermissions | undefined;
    if (!permissions || typeof permissions.query !== "function") return () => {};
  } catch {
    return () => {};
  }

  let status: LoopbackPermissionStatus;
  try {
    status = await permissions.query({ name: "loopback-network" });
  } catch {
    try {
      status = await permissions.query({ name: "local-network-access" });
    } catch {
      return () => {};
    }
  }

  if (
    typeof status.addEventListener !== "function" ||
    typeof status.removeEventListener !== "function"
  ) {
    return () => {};
  }

  let lastState = status.state;
  const listener = () => {
    if (status.state === lastState) return;
    lastState = status.state;
    onChange(lastState);
  };
  status.addEventListener("change", listener);
  return () => status.removeEventListener("change", listener);
}

/** Exhaustive UI model for every public SDK status arm. */
export function acceleratorStatusView(status: AcceleratorStatus): AcceleratorStatusView {
  if (status.available) {
    return {
      connected: true,
      label: "running",
      log: "Presto detected on loopback",
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
        log: "The browser blocked local access. Allow it in site permissions, then Retry",
        logLevel: "warn",
        showInstall: false,
        showPermissionHelp: true,
      };
    case "offline":
      return {
        connected: false,
        label: "not detected, in-browser",
        log: "Presto not detected, proving stays in-browser",
        logLevel: "warn",
        showInstall: true,
        showPermissionHelp: false,
      };
    case "error":
      return {
        connected: false,
        label: "health check error, in-browser",
        log: "Presto answered unexpectedly, proving in-browser",
        logLevel: "error",
        showInstall: false,
        showPermissionHelp: false,
      };
    case "version-mismatch":
      return {
        connected: false,
        label: "version mismatch, in-browser",
        log: "Presto's Aztec version is incompatible, proving in-browser",
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
  #inFlight = new Set<Promise<void>>();
  #permissionRefresh: Promise<void> | null = null;

  constructor(private readonly options: AcceleratorStatusControllerOptions) {}

  get displayed(): AcceleratorStatus | null {
    return this.#displayed;
  }

  refresh(checkOptions?: AcceleratorStatusCheckOptions): Promise<void> {
    const epoch = ++this.#epoch;
    this.options.setPending(true);
    const operation = (async () => {
      try {
        const status = await this.options.check(checkOptions);
        if (epoch !== this.#epoch) return;
        this.#displayed = status;
        this.options.render(status);
      } finally {
        if (epoch === this.#epoch) this.options.setPending(false);
      }
    })();
    this.#inFlight.add(operation);
    void operation.then(
      () => this.#inFlight.delete(operation),
      () => this.#inFlight.delete(operation),
    );
    return operation;
  }

  /**
   * Re-probe after a browser permission transition. If the old bounded check has not settled yet,
   * wait for it before forcing the new probe: SDK force-refresh deliberately joins an in-flight
   * same-generation probe, which would otherwise reproduce the stale post-prompt result.
   */
  refreshAfterPermissionChange(): Promise<void> {
    if (this.#permissionRefresh) return this.#permissionRefresh;

    const pending = [...this.#inFlight];
    if (pending.length > 0) {
      // Revoke every old operation's display ownership while the new permission state waits to probe.
      ++this.#epoch;
      this.options.setPending(true);
    }

    const refresh = Promise.allSettled(pending).then(() => this.refresh({ forceRefresh: true }));
    this.#permissionRefresh = refresh.finally(() => {
      this.#permissionRefresh = null;
    });
    return this.#permissionRefresh;
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
