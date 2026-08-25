// B7 (F14): the SDK's ONE typed error. The accelerator is an optimisation, so the prover degrades to
// WASM for every RECOGNISED transient/denial/version/capacity condition (see the fallback table in
// `accelerator-prover.ts`). What must NOT be masked is a caller MISCONFIGURATION — a `400 invalid_version`
// / `invalid_origin`, a `500` with an unrecognised code, or a status the SDK does not recognise as a
// transient/denial/capacity condition: silently falling back there would hide a real integration bug behind
// "slow but working". Those, and only those, reach the dApp as this typed error instead of the
// internal transport error (which is not part of the SDK's public surface).

/**
 * Thrown by {@link AcceleratorProver} proving when the accelerator returns an HTTP error that indicates a
 * MISCONFIGURATION rather than a transient/denial/capacity condition — i.e. a `400 invalid_version` /
 * `invalid_origin`, a `500` with an unrecognised code, or an unexpected HTTP status. Recognised conditions —
 * EVERY `403`, EVERY `408`/`413`/`429`/`503`, and `500 download_failed`/`prove_failed` — degrade to WASM and
 * never throw, so an unknown `403`/`503` code degrades rather than throwing.
 */
export class AcceleratorHttpError extends Error {
  /** HTTP status the accelerator returned. */
  readonly status: number;
  /** The stable server error code (e.g. `invalid_version`), when the body carried one. */
  readonly code?: string;

  constructor(status: number, code?: string, serverMessage?: string) {
    super(
      serverMessage ??
        `The Aztec Accelerator rejected the request with HTTP ${status}${
          code ? ` (${code})` : ""
        }. This usually means the SDK is misconfigured for this accelerator.`,
    );
    this.name = "AcceleratorHttpError";
    this.status = status;
    this.code = code;
  }
}

/**
 * Recover the server's stable error `code` (and human `message`) from a transport error's bounded
 * pre-read body. The accelerator returns its error body as `text/plain` carrying a JSON string (pinned
 * by the Rust test `prove_error_responses_stay_text_plain`), so the pre-read parses to a STRING — the
 * code must be `JSON.parse`d out of it. (A JSON content-type instead gives an object; both shapes are
 * handled so tests using `Response.json(...)` and production `text/plain` agree. An unreadable body
 * gives `undefined` → `{}` here — the HTTP status still governs classification.)
 */
export function parseServerError(data: unknown): { code?: string; message?: string } {
  let obj: { error?: unknown; message?: unknown } | undefined;
  if (typeof data === "string") {
    try {
      obj = JSON.parse(data);
    } catch {
      return {};
    }
  } else if (data && typeof data === "object") {
    obj = data as { error?: unknown; message?: unknown };
  }
  if (!obj) return {};
  return {
    code: typeof obj.error === "string" ? obj.error : undefined,
    message: typeof obj.message === "string" ? obj.message : undefined,
  };
}
