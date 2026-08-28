export type LandingAcceleratorStatus = "available" | "offline" | "error" | "permission-blocked";

type LoopbackRequestInit = RequestInit & { targetAddressSpace?: "loopback" };
type LoopbackRequest = Request & { readonly targetAddressSpace?: string };
type LoopbackPermissionName = "loopback-network" | "local-network-access";
type LoopbackPermissions = {
  query(descriptor: { name: LoopbackPermissionName }): Promise<{ state: PermissionState }>;
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

async function permissionDenied(): Promise<boolean> {
  let permissions: LoopbackPermissions | undefined;
  try {
    if (typeof navigator === "undefined") return false;
    permissions = navigator.permissions as unknown as LoopbackPermissions | undefined;
    if (!permissions || typeof permissions.query !== "function") return false;
  } catch {
    return false;
  }

  try {
    return (await permissions.query({ name: "loopback-network" })).state === "denied";
  } catch {
    try {
      return (await permissions.query({ name: "local-network-access" })).state === "denied";
    } catch {
      return false;
    }
  }
}

async function fetchBounded(url: string): Promise<Response> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), HEADER_TIMEOUT_MS);
  const init: LoopbackRequestInit = { redirect: "error", signal: ctrl.signal };
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

type Candidate = { reached: true; healthy: boolean } | { reached: false };

async function probe(url: string): Promise<Candidate> {
  try {
    const response = await fetchBounded(url);
    if (!response.ok) return { reached: true, healthy: false };
    return { reached: true, healthy: isRecognizedHealth(await readHealthBody(response)) };
  } catch {
    return { reached: false };
  }
}

/** Lightweight landing detector. It deliberately has no settled cache, so Retry is immediate. */
export async function detectAccelerator(options?: {
  httpsOnly?: boolean;
}): Promise<LandingAcceleratorStatus> {
  const urls = options?.httpsOnly
    ? ["https://127.0.0.1:59834/health"]
    : ["http://127.0.0.1:59833/health", "https://127.0.0.1:59834/health"];
  const results = await Promise.all(urls.map(probe));
  if (results.some((result) => result.reached && result.healthy)) return "available";
  if (results.some((result) => result.reached)) return "error";
  return (await permissionDenied()) ? "permission-blocked" : "offline";
}

export class LandingDetectionController {
  #epoch = 0;

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
}
