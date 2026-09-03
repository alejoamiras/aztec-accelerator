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
  showSecureConnectionHelp: boolean;
  secureConnectionTitle?: string;
  secureConnectionMessage?: string;
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
      label: "available",
      log: "Native accelerator detected on loopback",
      logLevel: "success",
      showInstall: false,
      showPermissionHelp: false,
      showSecureConnectionHelp: false,
    };
  }

  switch (status.reason) {
    case "permission-blocked":
      return {
        connected: false,
        label: "local access blocked",
        log: "The browser blocked local access; allow it in site permissions, then Retry",
        logLevel: "warn",
        showInstall: false,
        showPermissionHelp: true,
        showSecureConnectionHelp: false,
      };
    case "secure-connection-unavailable": {
      const explanation = {
        "https-disabled": {
          title: "Encrypted Connection is disabled",
          message:
            "Accelerator is running, but its HTTPS listener is off. Enable Encrypted Connection in Settings.",
        },
        "tls-or-trust-failure": {
          title: "Secure connection is not trusted",
          message:
            "Accelerator advertises HTTPS, but the browser could not establish it. Repair the local certificate trust setup.",
        },
        "accelerator-reachable": {
          title: "Accelerator is reachable",
          message:
            "The public health response hides HTTPS details. Check Encrypted Connection and local certificate trust.",
        },
        unconfirmed: {
          title: "Secure connection unavailable",
          message:
            "The diagnostic could not confirm Accelerator. It may be stopped or not installed, or the browser may have blocked local HTTP.",
        },
      }[status.diagnosis];
      return {
        connected: false,
        label: "secure connection unavailable, fallback: wasm",
        log: `${explanation.title}; falling back to WASM`,
        logLevel: "warn",
        showInstall: status.diagnosis === "unconfirmed",
        showPermissionHelp: false,
        showSecureConnectionHelp: true,
        secureConnectionTitle: explanation.title,
        secureConnectionMessage: explanation.message,
      };
    }
    case "offline":
      return {
        connected: false,
        label: "not detected, fallback: wasm",
        log: "Accelerator not detected, will fall back to WASM",
        logLevel: "warn",
        showInstall: true,
        showPermissionHelp: false,
        showSecureConnectionHelp: false,
      };
    case "error":
      return {
        connected: false,
        label: "health check error, fallback: wasm",
        log: "Accelerator answered unexpectedly; falling back to WASM",
        logLevel: "error",
        showInstall: false,
        showPermissionHelp: false,
        showSecureConnectionHelp: false,
      };
    case "version-mismatch":
      return {
        connected: false,
        label: "version mismatch, fallback: wasm",
        log: "Accelerator Aztec version is incompatible; falling back to WASM",
        logLevel: "warn",
        showInstall: false,
        showPermissionHelp: false,
        showSecureConnectionHelp: false,
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
  #secureRefresh: Promise<void> | null = null;

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

  /** Coalesce repeated secure-recovery clicks into one forced status refresh. */
  retrySecureConnection(): Promise<void> {
    if (this.#secureRefresh) return this.#secureRefresh;
    const refresh = this.refresh({ forceRefresh: true });
    this.#secureRefresh = refresh.finally(() => {
      this.#secureRefresh = null;
    });
    return this.#secureRefresh;
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

interface HttpSessionConsentControllerOptions {
  configure: () => void;
  refresh: () => Promise<void>;
  setConfirmationOpen: (open: boolean) => void;
  setPending: (pending: boolean) => void;
  announce: (message: string) => void;
}

/** Owns the explicit, non-persistent HTTP confirmation flow and suppresses duplicate activation. */
export class HttpSessionConsentController {
  #confirmationOpen = false;
  #operation: Promise<void> | null = null;

  constructor(private readonly options: HttpSessionConsentControllerOptions) {}

  request(): void {
    if (this.#confirmationOpen || this.#operation) return;
    this.#confirmationOpen = true;
    this.options.setConfirmationOpen(true);
  }

  cancel(): void {
    if (!this.#confirmationOpen || this.#operation) return;
    this.#confirmationOpen = false;
    this.options.setConfirmationOpen(false);
    this.options.announce("HTTP session fallback cancelled. HTTPS-only proving remains enabled.");
  }

  confirm(): Promise<void> {
    if (this.#operation) return this.#operation;
    if (!this.#confirmationOpen) return Promise.resolve();

    this.#confirmationOpen = false;
    this.options.setConfirmationOpen(false);
    this.options.setPending(true);
    this.options.announce("Enabling HTTP for this tab and checking Accelerator status.");

    let configured = false;
    const operation = Promise.resolve()
      .then(() => {
        this.options.configure();
        configured = true;
      })
      .then(() => this.options.refresh())
      .then(() => {
        this.options.announce("HTTP is allowed for this tab only. Accelerator status refreshed.");
      })
      .catch((error) => {
        this.options.announce(
          configured
            ? "HTTP is allowed for this tab, but the status refresh failed. WASM fallback remains available."
            : "HTTP was not enabled because configuration failed. HTTPS-only proving remains enabled.",
        );
        throw error;
      })
      .finally(() => {
        this.options.setPending(false);
        if (this.#operation === operation) this.#operation = null;
      });
    this.#operation = operation;
    return operation;
  }
}
