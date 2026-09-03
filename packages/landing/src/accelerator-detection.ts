export type LandingSecureConnectionDiagnosis =
  | "https-disabled"
  | "tls-or-trust-failure"
  | "accelerator-reachable"
  | "unconfirmed";

export type LandingAcceleratorStatus =
  | "available"
  | "error"
  | "permission-blocked"
  | {
      reason: "secure-connection-unavailable";
      diagnosis: LandingSecureConnectionDiagnosis;
    };

type LoopbackRequestInit = RequestInit & { targetAddressSpace?: "loopback" };
type LoopbackRequest = Request & { readonly targetAddressSpace?: string };
type LoopbackPermissionName = "loopback-network" | "local-network-access";
type LoopbackPermissionStatus = {
  readonly state: PermissionState;
  addEventListener?(type: "change", listener: () => void): void;
  removeEventListener?(type: "change", listener: () => void): void;
};
type LoopbackPermissions = {
  query(descriptor: { name: LoopbackPermissionName }): Promise<LoopbackPermissionStatus>;
};

const HEADER_TIMEOUT_MS = 2_000;
const BODY_TIMEOUT_MS = 2_000;
const HEALTH_MAX_BYTES = 64 * 1024;
const HEALTH = { status: "ok", api_version: 1 } as const;

export function supportsLoopbackTargetAddressSpace(): boolean {
  try {
    return (
      typeof Request !== "undefined" &&
      typeof Request.prototype === "object" &&
      "targetAddressSpace" in (Request.prototype as LoopbackRequest)
    );
  } catch {
    return false;
  }
}

async function queryLoopbackPermission(): Promise<LoopbackPermissionStatus | null> {
  let permissions: LoopbackPermissions | undefined;
  try {
    if (typeof navigator === "undefined") return null;
    permissions = navigator.permissions as unknown as LoopbackPermissions | undefined;
    if (!permissions || typeof permissions.query !== "function") return null;
  } catch {
    return null;
  }

  try {
    return await permissions.query({ name: "loopback-network" });
  } catch {
    try {
      return await permissions.query({ name: "local-network-access" });
    } catch {
      return null;
    }
  }
}

async function permissionDenied(): Promise<boolean> {
  return (await queryLoopbackPermission())?.state === "denied";
}

/** Observe a prompt decision that occurs after the bounded landing-page probe has timed out. */
export async function watchLoopbackPermissionChanges(
  onChange: (state: PermissionState) => void,
): Promise<() => void> {
  const status = await queryLoopbackPermission();
  if (
    !status ||
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
  return () => status.removeEventListener?.("change", listener);
}

async function fetchBounded(url: string): Promise<Response> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), HEADER_TIMEOUT_MS);
  const init: LoopbackRequestInit = { method: "GET", redirect: "error", signal: ctrl.signal };
  if (new URL(url).protocol === "http:" && supportsLoopbackTargetAddressSpace()) {
    init.targetAddressSpace = "loopback";
  }
  try {
    return await fetch(url, init);
  } finally {
    clearTimeout(timer);
  }
}

async function readHealthBody(response: Response): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) return undefined;
  let buffer = new Uint8Array(Math.min(HEALTH_MAX_BYTES, 4 * 1024));
  let total = 0;
  const deadline = performance.now() + BODY_TIMEOUT_MS;

  try {
    while (true) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) return undefined;
      let timer: ReturnType<typeof setTimeout> | undefined;
      const result = await Promise.race([
        reader.read(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error("body deadline")), remaining);
        }),
      ]).finally(() => clearTimeout(timer));
      if (result.done) break;
      if (total + result.value.byteLength > HEALTH_MAX_BYTES) return undefined;
      if (total + result.value.byteLength > buffer.byteLength) {
        const grown = new Uint8Array(
          Math.min(
            HEALTH_MAX_BYTES,
            Math.max(total + result.value.byteLength, buffer.byteLength * 2),
          ),
        );
        grown.set(buffer.subarray(0, total));
        buffer = grown;
      }
      buffer.set(result.value, total);
      total += result.value.byteLength;
    }
  } catch {
    return undefined;
  } finally {
    void reader.cancel().catch(() => {});
  }

  try {
    return JSON.parse(new TextDecoder().decode(buffer.subarray(0, total)));
  } catch {
    return undefined;
  }
}

function isRecognizedHealth(body: unknown): boolean {
  if (typeof body !== "object" || body === null || Array.isArray(body)) return false;
  const value = body as Record<string, unknown>;
  return value.status === HEALTH.status && value.api_version === HEALTH.api_version;
}

const DETAILED_HEALTH_KEYS = [
  "version",
  "aztec_version",
  "available_versions",
  "bb_available",
  "https_port",
] as const;

function hasAnyDetailedHealthField(body: Record<string, unknown>): boolean {
  return DETAILED_HEALTH_KEYS.some((key) => key in body);
}

function isValidHealth(body: unknown): body is Record<string, unknown> {
  if (!isRecognizedHealth(body)) return false;
  const value = body as Record<string, unknown>;
  return (
    (!("version" in value) || typeof value.version === "string") &&
    (!("aztec_version" in value) || typeof value.aztec_version === "string") &&
    (!("available_versions" in value) ||
      (Array.isArray(value.available_versions) &&
        value.available_versions.every((version) => typeof version === "string"))) &&
    (!("bb_available" in value) || typeof value.bb_available === "boolean") &&
    (!("https_port" in value) ||
      (typeof value.https_port === "number" &&
        Number.isInteger(value.https_port) &&
        value.https_port >= 1 &&
        value.https_port <= 65535))
  );
}

function isDetailedHealth(body: unknown): body is Record<string, unknown> {
  if (!isValidHealth(body)) return false;
  const value = body as Record<string, unknown>;
  return (
    typeof value.version === "string" &&
    typeof value.aztec_version === "string" &&
    Array.isArray(value.available_versions) &&
    value.available_versions.every((version) => typeof version === "string") &&
    typeof value.bb_available === "boolean"
  );
}

type Candidate =
  | { reached: true; healthy: boolean; body: unknown }
  | { reached: false; body?: never };

async function probe(url: string): Promise<Candidate> {
  try {
    const response = await fetchBounded(url);
    if (!response.ok) return { reached: true, healthy: false, body: undefined };
    const body = await readHealthBody(response);
    return { reached: true, healthy: isValidHealth(body), body };
  } catch {
    return { reached: false };
  }
}

/** Lightweight landing detector. It never enables HTTP; Retry is immediate because there is no cache. */
export async function detectAccelerator(): Promise<LandingAcceleratorStatus> {
  const secure = await probe("https://127.0.0.1:59834/health");
  if (secure.reached) return secure.healthy ? "available" : "error";
  if (await permissionDenied()) return "permission-blocked";

  // Informational only: one witness-free HTTP GET helps explain how to repair HTTPS. This result is
  // never treated as available and the landing page has no HTTP proving or consent path.
  const diagnostic = await probe("http://127.0.0.1:59833/health");
  if (!diagnostic.reached) {
    return (await permissionDenied())
      ? "permission-blocked"
      : { reason: "secure-connection-unavailable", diagnosis: "unconfirmed" };
  }
  if (!diagnostic.healthy) {
    return { reason: "secure-connection-unavailable", diagnosis: "unconfirmed" };
  }
  const recognized = diagnostic.body as Record<string, unknown>;
  if (hasAnyDetailedHealthField(recognized) && !isDetailedHealth(recognized)) {
    return { reason: "secure-connection-unavailable", diagnosis: "unconfirmed" };
  }
  if (!isDetailedHealth(diagnostic.body)) {
    return { reason: "secure-connection-unavailable", diagnosis: "accelerator-reachable" };
  }
  if (!("https_port" in diagnostic.body)) {
    return { reason: "secure-connection-unavailable", diagnosis: "https-disabled" };
  }
  const httpsPort = diagnostic.body.https_port;
  return {
    reason: "secure-connection-unavailable",
    diagnosis:
      typeof httpsPort === "number" &&
      Number.isInteger(httpsPort) &&
      httpsPort >= 1 &&
      httpsPort <= 65535
        ? "tls-or-trust-failure"
        : "unconfirmed",
  };
}

export class LandingDetectionController {
  #epoch = 0;
  #permissionRefresh: Promise<void> | null = null;

  constructor(
    private readonly check: () => Promise<LandingAcceleratorStatus>,
    private readonly render: (status: LandingAcceleratorStatus) => void,
    private readonly setPending: (pending: boolean) => void,
  ) {}

  async refresh(): Promise<void> {
    const epoch = ++this.#epoch;
    this.setPending(true);
    try {
      const status = await this.check();
      if (epoch === this.#epoch) this.render(status);
    } finally {
      if (epoch === this.#epoch) this.setPending(false);
    }
  }

  /** Coalesce browser permission events into one uncached probe with fresh display ownership. */
  refreshAfterPermissionChange(): Promise<void> {
    if (this.#permissionRefresh) return this.#permissionRefresh;
    const refresh = this.refresh();
    this.#permissionRefresh = refresh.finally(() => {
      this.#permissionRefresh = null;
    });
    return this.#permissionRefresh;
  }
}
